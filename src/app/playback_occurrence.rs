//! Playback identity, terminal handling, and once-per-occurrence reporting.

use crate::models::{PlaybackId, QueueEntryId};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Selection {
    New,
    Resume,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Restored,
    Active,
    Failed,
    Ended,
}

struct Occurrence {
    id: PlaybackId,
    entry: QueueEntryId,
    song_id: String,
    phase: Phase,
    reported: bool,
    scrobbled: bool,
}

#[derive(Default)]
pub(super) struct PlaybackOccurrence {
    current: Option<Occurrence>,
}

impl PlaybackOccurrence {
    /// True means a new playback, so any old listening event must be finished.
    pub(super) fn select(
        &mut self,
        entry: QueueEntryId,
        song_id: &str,
        selection: Selection,
    ) -> bool {
        if selection == Selection::Resume {
            if let Some(current) = self.current.as_mut().filter(|current| {
                current.entry == entry
                    && current.song_id == song_id
                    && current.phase != Phase::Ended
            }) {
                current.phase = Phase::Active;
                return false;
            }
        }
        self.current = Some(Occurrence {
            id: PlaybackId::new(),
            entry,
            song_id: song_id.into(),
            phase: Phase::Active,
            reported: false,
            scrobbled: false,
        });
        true
    }

    pub(super) fn restore(&mut self, entry: QueueEntryId, song_id: &str) {
        self.select(entry, song_id, Selection::New);
        self.current.as_mut().unwrap().phase = Phase::Restored;
    }

    pub(super) fn id(&self) -> Option<PlaybackId> {
        self.current.as_ref().map(|current| current.id)
    }

    pub(super) fn active(&self) -> bool {
        self.current
            .as_ref()
            .is_some_and(|current| current.phase == Phase::Active)
    }

    pub(super) fn claim_terminal(&mut self, expected: Option<PlaybackId>, ended: bool) -> bool {
        let Some(current) = self
            .current
            .as_mut()
            .filter(|current| current.phase == Phase::Active && Some(current.id) == expected)
        else {
            return false;
        };
        current.phase = if ended { Phase::Ended } else { Phase::Failed };
        true
    }

    pub(super) fn claim_report(&mut self, song_id: &str) -> bool {
        let Some(current) = self.current.as_mut().filter(|current| {
            current.song_id == song_id && current.phase == Phase::Active && !current.reported
        }) else {
            return false;
        };
        current.reported = true;
        true
    }

    pub(super) fn claim_scrobble(&mut self, song_id: &str) -> bool {
        let Some(current) = self.current.as_mut().filter(|current| {
            current.song_id == song_id && current.phase != Phase::Restored && !current.scrobbled
        }) else {
            return false;
        };
        current.scrobbled = true;
        true
    }

    pub(super) fn clear(&mut self) {
        self.current = None;
    }
}

#[cfg(test)]
mod tests;
