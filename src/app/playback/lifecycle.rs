//! 播放结束处理和断线重连生命周期。

use super::reconnect::{ReconnectDecision, ReconnectPoll, RetryKind};
use super::start::StartIntent;
use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn handle_playback_end(&mut self) {
        self.sync_gapless_transition();
        self.handle_playback_end_for_intent(self.start_resolution.pending());
    }

    pub(in crate::app) fn handle_playback_end_for_intent(&mut self, resolving: bool) {
        let playback = self.audio.state();
        if self.state.now_playing.is_none() {
            return;
        }
        let occurrence = self.state.occurrence.id();
        if playback.ended && playback.error.is_none() {
            if self.state.occurrence.claim_terminal(occurrence, true) {
                self.freeze_next_preload();
                self.ensure_scrobble_on_end();
                self.finish_listening_event(true);
                self.cache_measured_loudness(playback.loudness_db, playback.loudness_peak);
            }
            if self.state.occurrence.claim_advance(resolving) && !self.play_next(true) {
                self.maybe_start_auto_continue();
            }
            return;
        }
        if resolving || !self.state.occurrence.active() {
            return;
        }
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
        }
    }

    /// 播放中断后安排一次自动重连。只有网络/服务端类失败才重试；
    /// 解码和本地输出问题交给调用方处理；次数用尽时在这里报错，避免重复提示。
    pub(in crate::app) fn schedule_reconnect(
        &mut self,
        failure: Option<StreamFailure>,
        position: Duration,
    ) -> bool {
        let Some(occurrence) = self.state.occurrence.id() else {
            return false;
        };
        let policy = self
            .playback_source
            .as_ref()
            .map(|api| api.playback_policy())
            .unwrap_or_default();
        let refreshing = policy.refreshable_addresses
            && matches!(
                failure,
                Some(StreamFailure::Http(401 | 403) | StreamFailure::ResourceExpired)
            );
        if policy.stop_on_rate_limit && failure == Some(StreamFailure::Http(429)) {
            self.reconnect.cancel();
            self.notify_error(t!("Bilibili request rate limited. Retry later."));
            return true;
        }
        let attempts = if refreshing {
            1
        } else {
            PLAYBACK_RECONNECT_ATTEMPTS
        };
        let decision = if refreshing {
            let now = std::time::Instant::now();
            if failure == Some(StreamFailure::ResourceExpired) {
                let decision = self
                    .reconnect
                    .schedule_expired_resource(occurrence, position, now);
                if matches!(decision, ReconnectDecision::Retry { .. }) {
                    self.poll_playback_reconnect();
                    return true;
                }
                decision
            } else {
                self.reconnect
                    .schedule_resource_refresh(occurrence, position, now)
            }
        } else {
            self.reconnect
                .schedule(occurrence, failure, position, std::time::Instant::now())
        };
        self.report_reconnect_decision(failure, decision, attempts, position)
    }

    pub(in crate::app) fn report_reconnect_decision(
        &mut self,
        failure: Option<StreamFailure>,
        decision: ReconnectDecision,
        attempts: u8,
        position: Duration,
    ) -> bool {
        let Some(kind) = failure else {
            return false;
        };
        let (attempt, delay) = match decision {
            ReconnectDecision::DoNotRetry => return false,
            ReconnectDecision::Exhausted => {
                self.notify_error(t!(
                    "%{summary} — gave up after %{attempts} retries. %{cause}",
                    summary = kind.summary(),
                    attempts = attempts,
                    cause = kind.cause()
                ));
                return true;
            }
            ReconnectDecision::Retry { attempt, delay } => (attempt, delay),
        };
        log::warn!(
            "playback interrupted ({kind:?}); scheduling reconnect {attempt}/{attempts} in {}ms from {}ms",
            delay.as_millis(),
            position.as_millis()
        );
        self.notify_warning(t!(
            "%{summary} — reconnecting (%{attempt}/%{attempts})…",
            summary = kind.summary(),
            attempt = attempt,
            attempts = attempts
        ));
        true
    }

    /// 由 UI 循环调用：到点后从断点重新拉流，并在播放恢复正常后清零计数。
    pub(in crate::app) fn poll_playback_reconnect(&mut self) {
        if self.start_resolution.pending() {
            return;
        }
        let playback = self.audio.progress();
        let index = self
            .state
            .queue_index
            .filter(|index| *index < self.state.queue.len());
        let occurrence = self.state.occurrence.id();
        match self.reconnect.poll(
            std::time::Instant::now(),
            occurrence,
            self.playback_source.is_some() && index.is_some(),
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
                    let intent = match plan.kind {
                        RetryKind::Network => StartIntent::Reconnect,
                        RetryKind::ResourceRefresh => StartIntent::ResourceRefresh,
                    };
                    if !self.play_queue_index_at(index, plan.position, intent) {
                        self.reconnect.cancel();
                    }
                }
            }
        }
    }
}
