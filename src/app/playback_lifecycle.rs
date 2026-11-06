//! 播放结束处理和断线重连生命周期。

use super::playback_reconnect::{ReconnectDecision, ReconnectPoll};
use super::*;

impl NavidromeApp {
    pub(super) fn handle_playback_end(&mut self) {
        self.sync_gapless_transition();
        let playback = self.audio.state();
        if !self.state.occurrence.active() || self.state.now_playing.is_none() {
            return;
        }
        let occurrence = self.state.occurrence.id();
        if let Some(error) = playback.error {
            self.freeze_next_preload();
            if !self.state.occurrence.claim_terminal(occurrence, false) {
                return;
            }
            self.cancel_next_preload();
            log::error!("playback error reported to UI: {error}");
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
            self.freeze_next_preload();
            if !self.state.occurrence.claim_terminal(occurrence, true) {
                return;
            }
            self.ensure_scrobble_on_end();
            self.finish_listening_event(true);
            self.cache_measured_loudness(playback.loudness_db, playback.loudness_peak);
            if !self.play_next(true) {
                self.maybe_start_auto_continue();
            }
        }
    }

    /// 播放中断后安排一次自动重连。只有网络/服务端类失败才重试；
    /// 解码和本地输出问题交给调用方处理；次数用尽时在这里报错，避免重复提示。
    pub(super) fn schedule_reconnect(
        &mut self,
        failure: Option<StreamFailure>,
        position: Duration,
    ) -> bool {
        let Some(occurrence) = self.state.occurrence.id() else {
            return false;
        };
        let decision =
            self.reconnect
                .schedule(occurrence, failure, position, std::time::Instant::now());
        let Some(kind) = failure else {
            return false;
        };
        let (attempt, delay) = match decision {
            ReconnectDecision::DoNotRetry => return false,
            ReconnectDecision::Exhausted => {
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
        true
    }

    /// 由 UI 循环调用：到点后从断点重新拉流，并在播放恢复正常后清零计数。
    pub(super) fn poll_playback_reconnect(&mut self) {
        let playback = self.audio.progress();
        let index = self
            .state
            .queue_index
            .filter(|index| *index < self.state.queue.len());
        let occurrence = self.state.occurrence.id();
        match self.reconnect.poll(
            std::time::Instant::now(),
            occurrence,
            self.api.is_some() && index.is_some(),
            &playback,
        ) {
            ReconnectPoll::Idle => {}
            ReconnectPoll::Recovered => {
                log::info!("playback recovered; clearing reconnect attempts");
            }
            ReconnectPoll::Retry(plan) => {
                self.cancel_next_preload();
                if self.state.occurrence.id() != occurrence {
                    self.reconnect.cancel();
                    return;
                }
                if let Some(index) = self.state.queue_index {
                    log::info!(
                        "reconnecting playback; attempt={} index={index} position_ms={}",
                        plan.attempt,
                        plan.position.as_millis()
                    );
                    self.play_queue_index_at(index, plan.position);
                }
            }
        }
    }
}
