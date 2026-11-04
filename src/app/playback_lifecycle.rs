//! 播放会话持久化、播放结束处理和断线重连生命周期。

use super::*;

impl NavidromeApp {
    /// 持久化当前播放会话（队列、索引、位置、模式、随机历史）。
    pub(super) fn save_session(&mut self) {
        let Some(api) = self.api.as_ref() else { return };
        debug_assert_eq!(self.state.queue.len(), self.state.queue_origins.len());
        let position = self.audio.state().position;
        let (shuffle_played, shuffle_forward) = self.state.shuffle_history.snapshot();
        let api_server_id = self
            .config
            .active_server_id()
            .unwrap_or_default()
            .to_string();
        let session = PlaybackSession {
            server_url: api.base_url().to_string(),
            queue: self.state.queue.clone(),
            queue_origins: self.state.queue_origins.clone(),
            queue_index: self.state.queue_index,
            position_secs: position.as_secs_f64(),
            playback_modes: self.state.playback_modes,
            shuffle_played,
            shuffle_forward,
        };
        let server_id = api_server_id.to_string();
        if let Err(error) = config::save_session(Some(&server_id), &session) {
            log::warn!("failed to save playback session: {error:#}");
        }
    }

    /// 位置变化节流保存（每 5 秒一次，避免频繁写盘）。
    pub(super) fn maybe_save_session(&mut self) {
        if self.state.queue.is_empty() {
            return;
        }
        let now = std::time::Instant::now();
        if now.duration_since(self.last_session_save) < Duration::from_secs(5) {
            return;
        }
        self.last_session_save = now;
        self.save_session();
    }

    pub(super) fn handle_playback_end(&mut self) {
        self.sync_gapless_transition();
        let playback = self.audio.state();
        if self.state.ended_handled || self.state.now_playing.is_none() {
            return;
        }
        if let Some(error) = playback.error {
            self.cancel_next_preload();
            log::error!("playback error reported to UI: {error}");
            self.state.ended_handled = true;
            if self.schedule_reconnect(playback.failure, playback.position) {
                return;
            }
            // Opus 源解码失败时给出可操作的提示（而不是误导性的
            // “格式未识别”）：说明需要服务器转码，并指向两案。
            let undecodable_source = self.state.now_playing.as_ref().is_some_and(|song| {
                TranscodingQuality::is_undecodable_source(
                    song.suffix.as_deref(),
                    song.content_type.as_deref(),
                )
            });
            let error = if undecodable_source {
                t!(
                    "%{error} Opus tracks must be transcoded by the server: enable transcoding in Navidrome, or pick a lower playback quality.",
                    error = error
                )
                .to_string()
            } else {
                error
            };
            self.notify_error(error);
        } else if playback.ended {
            self.state.ended_handled = true;
            self.ensure_scrobble_on_end();
            self.finish_listening_event(true);
            self.cache_measured_loudness(playback.loudness_db, playback.loudness_peak);
            if !self.play_next(true) {
                self.maybe_start_auto_continue();
            }
        }
    }

    /// 播放中断后安排一次自动重连。只有网络/服务端类失败才重试；
    /// 解码、本地输出问题以及重试次数用尽时返回 false。
    pub(super) fn schedule_reconnect(
        &mut self,
        failure: Option<StreamFailure>,
        position: Duration,
    ) -> bool {
        let Some(song) = self.state.now_playing.clone() else {
            return false;
        };
        let Some(kind) = failure else {
            return false;
        };
        let previous = self
            .reconnect_attempts
            .as_ref()
            .filter(|(id, _, _)| id == &song.id)
            .map_or(0, |(_, count, _)| *count);
        let (attempt, delay) =
            match reconnect_decision(failure, previous, PLAYBACK_RECONNECT_ATTEMPTS) {
                ReconnectDecision::DoNotRetry => return false,
                ReconnectDecision::Exhausted => {
                    self.reconnect = None;
                    self.reconnect_attempts = None;
                    self.notify_error(t!(
                        "%{summary} — gave up after %{attempts} retries. %{cause}",
                        summary = kind.summary(),
                        attempts = PLAYBACK_RECONNECT_ATTEMPTS,
                        cause = kind.cause()
                    ));
                    return true;
                }
                ReconnectDecision::Retry { attempt, delay } => (attempt, delay),
            };
        log::warn!(
            "playback interrupted ({kind:?}); scheduling reconnect {attempt}/{PLAYBACK_RECONNECT_ATTEMPTS} in {}ms from {}ms",
            delay.as_millis(),
            position.as_millis()
        );
        self.notify_warning(t!(
            "%{summary} — reconnecting (%{attempt}/%{attempts})…",
            summary = kind.summary(),
            attempt = attempt,
            attempts = PLAYBACK_RECONNECT_ATTEMPTS
        ));
        self.reconnect_attempts = Some((song.id.clone(), attempt, position));
        self.reconnect = Some(Reconnect {
            at: std::time::Instant::now() + delay,
            attempt,
            position,
            song_id: song.id,
        });
        true
    }

    /// 由 UI 循环调用：到点后从断点重新拉流，并在播放恢复正常后清零计数。
    pub(super) fn poll_playback_reconnect(&mut self) {
        let playback = self.audio.state();
        // 只有播放真的重新前进了一段（而不是刚发出 Play 就被当成成功）才清零
        // 重试计数，否则连续失败会被无限重试。
        if self.reconnect.is_none() && playback.error.is_none() {
            let recovered = self
                .reconnect_attempts
                .as_ref()
                .is_some_and(|(id, _, failed_at)| {
                    self.state
                        .now_playing
                        .as_ref()
                        .is_some_and(|song| &song.id == id)
                        && playback.position > failed_at.saturating_add(RECOVERY_CONFIRMATION)
                });
            if recovered {
                log::info!("playback recovered; clearing reconnect attempts");
                self.reconnect_attempts = None;
            }
        }
        let Some(plan) = self.reconnect.as_ref() else {
            return;
        };
        if std::time::Instant::now() < plan.at {
            return;
        }
        let Some(plan) = self.reconnect.take() else {
            return;
        };
        // 用户已经切歌或停止时宁可不重连。
        let still_current = self
            .state
            .now_playing
            .as_ref()
            .is_some_and(|song| song.id == plan.song_id);
        if !still_current || self.api.is_none() {
            self.reconnect_attempts = None;
            return;
        }
        if let Some(index) = self.state.queue_index {
            log::info!(
                "reconnecting playback; attempt={} index={index} position_ms={}",
                plan.attempt,
                plan.position.as_millis()
            );
            self.play_queue_index_at(index, plan.position);
            self.state.ended_handled = false;
        }
    }
}
