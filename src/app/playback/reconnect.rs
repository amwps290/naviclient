//! Automatic stream retry deadlines and confirmation of actual playback recovery.

use std::time::{Duration, Instant};

use crate::audio::{PlaybackState, StreamFailure, PLAYBACK_RECONNECT_ATTEMPTS};
use crate::models::PlaybackId;

const RECOVERY_CONFIRMATION: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(in crate::app) struct PlaybackReconnect {
    phase: ReconnectPhase,
}

#[derive(Default)]
enum ReconnectPhase {
    #[default]
    Idle,
    Waiting {
        context: RetryContext,
        at: Instant,
    },
    Recovering(RetryContext),
}

struct RetryContext {
    occurrence: PlaybackId,
    attempt: u8,
    position: Duration,
}

#[derive(Debug, PartialEq, Eq)]
pub(in crate::app) struct ReconnectAttempt {
    pub(in crate::app) attempt: u8,
    pub(in crate::app) position: Duration,
}

#[derive(Debug, PartialEq, Eq)]
pub(in crate::app) enum ReconnectPoll {
    Idle,
    Recovered,
    Retry(ReconnectAttempt),
}

impl PlaybackReconnect {
    fn context(&self) -> Option<&RetryContext> {
        match &self.phase {
            ReconnectPhase::Idle => None,
            ReconnectPhase::Waiting { context, .. } | ReconnectPhase::Recovering(context) => {
                Some(context)
            }
        }
    }

    /// Explicit playback intent cancels both a deadline and its retry budget.
    pub(in crate::app) fn cancel(&mut self) {
        self.phase = ReconnectPhase::Idle;
    }

    pub(in crate::app) fn schedule(
        &mut self,
        occurrence: PlaybackId,
        failure: Option<StreamFailure>,
        position: Duration,
        now: Instant,
    ) -> ReconnectDecision {
        self.schedule_with_budget(
            occurrence,
            failure,
            position,
            now,
            PLAYBACK_RECONNECT_ATTEMPTS,
        )
    }

    /// A CDN rejection can indicate an expired signed URL. Resolve once; an
    /// API permission/region/auth failure stops before another audio request.
    pub(in crate::app) fn schedule_resource_refresh(
        &mut self,
        occurrence: PlaybackId,
        position: Duration,
        now: Instant,
    ) -> ReconnectDecision {
        self.schedule_with_budget(occurrence, Some(StreamFailure::Connect), position, now, 1)
    }

    fn schedule_with_budget(
        &mut self,
        occurrence: PlaybackId,
        failure: Option<StreamFailure>,
        position: Duration,
        now: Instant,
        budget: u8,
    ) -> ReconnectDecision {
        let previous = self
            .context()
            .filter(|context| context.occurrence == occurrence)
            .map_or(0, |context| context.attempt);
        let decision = reconnect_decision(failure, previous, budget);
        self.phase = match decision {
            ReconnectDecision::Retry { attempt, delay } => ReconnectPhase::Waiting {
                context: RetryContext {
                    occurrence,
                    attempt,
                    position,
                },
                at: now + delay,
            },
            ReconnectDecision::DoNotRetry | ReconnectDecision::Exhausted => ReconnectPhase::Idle,
        };
        decision
    }

    pub(in crate::app) fn poll(
        &mut self,
        now: Instant,
        occurrence: Option<PlaybackId>,
        can_play: bool,
        playback: &PlaybackState,
    ) -> ReconnectPoll {
        let Some(context) = self.context() else {
            return ReconnectPoll::Idle;
        };
        if occurrence != Some(context.occurrence) || !can_play {
            self.cancel();
            return ReconnectPoll::Idle;
        }

        match std::mem::take(&mut self.phase) {
            ReconnectPhase::Waiting { context, at } if now >= at => {
                let attempt = ReconnectAttempt {
                    attempt: context.attempt,
                    position: context.position,
                };
                self.phase = ReconnectPhase::Recovering(context);
                ReconnectPoll::Retry(attempt)
            }
            ReconnectPhase::Recovering(context)
                if playback.active
                    && !playback.paused
                    && playback.error.is_none()
                    && playback.position
                        > context.position.saturating_add(RECOVERY_CONFIRMATION) =>
            {
                ReconnectPoll::Recovered
            }
            phase => {
                self.phase = phase;
                ReconnectPoll::Idle
            }
        }
    }
}

/// A retryable failure waits before restarting the stream at its failed position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum ReconnectDecision {
    DoNotRetry,
    Exhausted,
    Retry { attempt: u8, delay: Duration },
}

fn reconnect_decision(
    failure: Option<StreamFailure>,
    previous_attempts: u8,
    max_attempts: u8,
) -> ReconnectDecision {
    if !failure.is_some_and(StreamFailure::is_retryable) {
        return ReconnectDecision::DoNotRetry;
    }
    let attempt = previous_attempts.saturating_add(1);
    if attempt > max_attempts {
        return ReconnectDecision::Exhausted;
    }
    let delay = match attempt {
        1 => Duration::from_millis(1500),
        _ => Duration::from_secs(4),
    };
    ReconnectDecision::Retry { attempt, delay }
}

#[cfg(test)]
mod tests;
