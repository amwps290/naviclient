//! Read-only queue target selection and prepared forward/backward history.

use std::collections::HashSet;

use rand::{seq::SliceRandom, Rng};

use super::logic::{advance_index, next_queue_index, ShuffleHistory};
use crate::app::shell::state::AppState;
use crate::models::{PlayModes, RepeatMode};

pub(in crate::app) enum QueueStep {
    KeepHistory(usize),
    WithHistory {
        index: usize,
        history: ShuffleHistory,
    },
}

/// A borrowed projection: planning cannot change the current queue or history.
pub(in crate::app) struct QueueNavigation<'a> {
    current: Option<usize>,
    len: usize,
    modes: PlayModes,
    history: &'a ShuffleHistory,
}

impl AppState {
    pub(in crate::app) fn queue_navigation(&self) -> QueueNavigation<'_> {
        QueueNavigation {
            current: self.queue_index,
            len: self.queue.len(),
            modes: self.playback_modes,
            history: &self.shuffle_history,
        }
    }
}

impl QueueNavigation<'_> {
    pub(in crate::app) fn next_index(
        &self,
        natural_end: bool,
        reserved: Option<usize>,
        rng: &mut impl Rng,
    ) -> Option<usize> {
        next_queue_index(
            self.modes,
            self.current?,
            self.len,
            natural_end,
            self.history.forward.last().copied(),
            reserved,
            || self.random_index(rng),
        )
    }

    pub(in crate::app) fn next_playback(
        &self,
        natural_end: bool,
        reserved: Option<usize>,
        rng: &mut impl Rng,
    ) -> Option<QueueStep> {
        let index = self.next_index(natural_end, reserved, rng)?;
        let mut history = self.history.clone();
        if self.modes.shuffle && history.restore_forward().is_none() {
            history.advance(index);
        }
        Some(QueueStep::WithHistory { index, history })
    }

    pub(in crate::app) fn previous_playback(&self) -> Option<QueueStep> {
        let current = self.current?;
        if self.len == 0 {
            return None;
        }
        if self.modes.shuffle {
            let mut history = self.history.clone();
            let index = history.previous()?;
            return Some(QueueStep::WithHistory { index, history });
        }
        advance_index(
            current,
            self.len,
            false,
            self.modes.repeat != RepeatMode::Off,
        )
        .map(QueueStep::KeepHistory)
    }

    fn random_index(&self, rng: &mut impl Rng) -> usize {
        let current = self.current.unwrap_or(0);
        if self.len <= 1 {
            return current;
        }
        let recent: HashSet<_> = self.history.recent((self.len - 1).min(8)).collect();
        let mut candidates: Vec<_> = (0..self.len)
            .filter(|index| !recent.contains(index))
            .collect();
        if candidates.is_empty() {
            candidates = (0..self.len).filter(|index| *index != current).collect();
        }
        candidates.choose(rng).copied().unwrap_or(current)
    }
}

#[cfg(test)]
mod tests;
