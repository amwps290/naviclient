//! Short-lived undo state for destructive queue edits and reorders.

use std::time::{Duration, Instant};

use rust_i18n::t;

use super::logic::ShuffleHistory;
use crate::models::PlaybackQueue;
use crate::source::SourceId;

pub(in crate::app) const QUEUE_UNDO_TTL: Duration = Duration::from_secs(8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum QueueUndoKind {
    Removed(usize),
    ClearedAfter,
    ClearedAll,
    Reordered,
}

impl QueueUndoKind {
    pub(in crate::app) fn label(self) -> String {
        match self {
            Self::Removed(1) => t!("Removed 1 track from queue").to_string(),
            Self::Removed(count) => {
                t!("Removed %{count} tracks from queue", count = count).to_string()
            }
            Self::ClearedAfter => t!("Cleared upcoming tracks").to_string(),
            Self::ClearedAll => t!("Cleared queue").to_string(),
            Self::Reordered => t!("Reordered queue").to_string(),
        }
    }
}

pub(in crate::app) struct QueueUndoEntry {
    pub(in crate::app) source: Option<SourceId>,
    pub(in crate::app) queue: PlaybackQueue,
    pub(in crate::app) queue_index: Option<usize>,
    pub(in crate::app) shuffle_history: ShuffleHistory,
    pub(in crate::app) kind: QueueUndoKind,
    pub(in crate::app) expires_at: Instant,
    /// True when the edit stopped playback or displaced the currently playing entry.
    pub(in crate::app) displaced_current: bool,
    pub(in crate::app) resume_position: Option<Duration>,
}

pub(in crate::app) struct QueueUndoRecord<'a> {
    pub(in crate::app) source: Option<SourceId>,
    pub(in crate::app) queue: &'a PlaybackQueue,
    pub(in crate::app) queue_index: Option<usize>,
    pub(in crate::app) shuffle_history: &'a ShuffleHistory,
    pub(in crate::app) kind: QueueUndoKind,
    pub(in crate::app) displaced_current: bool,
    pub(in crate::app) resume_position: Option<Duration>,
    pub(in crate::app) now: Instant,
}

#[derive(Default)]
pub(in crate::app) struct QueueUndoState {
    entry: Option<QueueUndoEntry>,
}

impl QueueUndoState {
    pub(in crate::app) fn record(&mut self, record: QueueUndoRecord<'_>) {
        self.entry = Some(QueueUndoEntry {
            source: record.source,
            queue: record.queue.clone_with_identities(),
            queue_index: record.queue_index,
            shuffle_history: record.shuffle_history.clone(),
            kind: record.kind,
            expires_at: record.now + QUEUE_UNDO_TTL,
            displaced_current: record.displaced_current,
            resume_position: record.resume_position,
        });
    }

    pub(in crate::app) fn active(&self, now: Instant) -> Option<&QueueUndoEntry> {
        self.entry.as_ref().filter(|entry| now < entry.expires_at)
    }

    pub(in crate::app) fn take(
        &mut self,
        now: Instant,
        current_source: Option<&SourceId>,
    ) -> Option<QueueUndoEntry> {
        let entry = self.entry.take()?;
        if now >= entry.expires_at || entry.source.as_ref() != current_source {
            return None;
        }
        Some(entry)
    }

    /// Clear expired entries; returns true if a visible banner just expired.
    pub(in crate::app) fn prune(&mut self, now: Instant) -> bool {
        if self
            .entry
            .as_ref()
            .is_some_and(|entry| now >= entry.expires_at)
        {
            self.entry = None;
            return true;
        }
        false
    }

    pub(in crate::app) fn clear(&mut self) {
        self.entry = None;
    }
}

/// Target index when moving a queue item directly after the current track.
pub(in crate::app) fn queue_move_to_next_target(
    index: usize,
    current: Option<usize>,
    len: usize,
) -> Option<usize> {
    if index >= len || len <= 1 {
        return None;
    }
    match current {
        Some(current) if current < len => {
            if index == current || index == current + 1 {
                None
            } else if index > current {
                Some(current + 1)
            } else {
                Some(current)
            }
        }
        _ => (index > 0).then_some(0),
    }
}

#[cfg(test)]
mod tests;
