//! Current stream preparation identity, wait timing, and explicit pause intent.

use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PreparationKind {
    Play,
    Seek,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PreparationToken {
    pub(super) generation: u64,
    pub(super) kind: PreparationKind,
    pub(super) position: Duration,
}

struct PendingPreparation {
    token: PreparationToken,
    since: Instant,
}

#[derive(Default)]
pub(super) struct PreparationState {
    generation: u64,
    pending: Option<PendingPreparation>,
    paused: bool,
}

impl PreparationState {
    pub(super) fn begin(
        &mut self,
        kind: PreparationKind,
        position: Duration,
        now: Instant,
    ) -> PreparationToken {
        self.generation = self.generation.wrapping_add(1);
        let token = PreparationToken {
            generation: self.generation,
            kind,
            position,
        };
        self.pending = Some(PendingPreparation { token, since: now });
        // Selecting a track plays it. Seeking and changing output devices keep
        // the most recent explicit pause/resume intent, independent of Player.
        if kind == PreparationKind::Play {
            self.paused = false;
        }
        token
    }

    /// A result is accepted exactly once, only for the still-current request.
    /// A stale success or failure cannot clear the newer request's wait state.
    pub(super) fn complete(&mut self, token: PreparationToken) -> bool {
        if self
            .pending
            .as_ref()
            .is_none_or(|pending| pending.token != token)
        {
            return false;
        }
        self.pending = None;
        true
    }

    pub(super) fn stop(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending = None;
        self.paused = false;
    }

    pub(super) fn preparing(&self) -> bool {
        self.pending.is_some()
    }

    pub(super) fn elapsed(&self, now: Instant) -> Option<Duration> {
        self.pending
            .as_ref()
            .map(|pending| now.saturating_duration_since(pending.since))
    }

    pub(super) fn paused(&self) -> bool {
        self.paused
    }

    pub(super) fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }
}

#[cfg(test)]
mod tests;
