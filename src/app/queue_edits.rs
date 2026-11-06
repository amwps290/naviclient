//! Owned queue additions, selected index, and shuffle history change together.

use super::queue_logic::{
    history_index_after_insert, queue_index_after_move, queue_index_after_remove, ShuffleHistory,
};
use super::song_scope::SongScope;
use super::song_selection::SongSelection;
use super::state::AppState;
use crate::models::{PlaybackQueue, QueueEntry, QueueOrigin, Song};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum QueueRemoval {
    Missing,
    KeptCurrent,
    PlayReplacement(usize),
    Stop,
}

pub(super) struct QueueEdits<'a> {
    queue: &'a mut PlaybackQueue,
    index: &'a mut Option<usize>,
    history: &'a mut ShuffleHistory,
    selection: &'a mut SongSelection,
}

impl AppState {
    pub(super) fn edit_queue(&mut self) -> QueueEdits<'_> {
        QueueEdits {
            queue: &mut self.queue,
            index: &mut self.queue_index,
            history: &mut self.shuffle_history,
            selection: &mut self.song_selection,
        }
    }
}

impl QueueEdits<'_> {
    pub(super) fn insert_next(&mut self, songs: Vec<Song>) -> bool {
        if songs.is_empty() {
            return false;
        }
        let old_len = self.queue.len();
        let inserted = songs.len();
        let insert_at = self
            .index
            .map_or(0, |index| index.saturating_add(1).min(self.queue.len()));
        self.queue.insert(insert_at, songs, QueueOrigin::Manual);
        self.remap(old_len, |index| {
            Some(history_index_after_insert(index, insert_at, inserted))
        });
        self.select_first_if_empty(old_len == 0);
        true
    }

    pub(super) fn append(&mut self, songs: Vec<Song>, origin: QueueOrigin) -> bool {
        self.append_entries(songs.into_iter().map(|song| QueueEntry::new(song, origin)))
    }

    pub(super) fn append_entries(&mut self, entries: impl IntoIterator<Item = QueueEntry>) -> bool {
        let entries: Vec<_> = entries.into_iter().collect();
        if entries.is_empty() {
            return false;
        }
        let old_len = self.queue.len();
        self.queue.append_entries(entries);
        self.remap(old_len, Some);
        self.select_first_if_empty(old_len == 0);
        true
    }

    fn select_first_if_empty(&mut self, was_empty: bool) {
        // A queue selection enables Play, but does not create a playback
        // occurrence, change now_playing, or start an audio command.
        if was_empty {
            *self.index = Some(0);
        }
    }

    pub(super) fn remove(&mut self, index: usize) -> QueueRemoval {
        let old_len = self.queue.len();
        if self.queue.remove(index).is_none() {
            return QueueRemoval::Missing;
        }
        let removed_current = *self.index == Some(index);
        self.remap(old_len, |current| {
            if current == index {
                None
            } else {
                Some(if current > index {
                    current - 1
                } else {
                    current
                })
            }
        });
        *self.index = queue_index_after_remove(index, *self.index, self.queue.len());
        if !removed_current {
            QueueRemoval::KeptCurrent
        } else if let Some(next) = *self.index {
            QueueRemoval::PlayReplacement(next)
        } else {
            QueueRemoval::Stop
        }
    }

    pub(super) fn move_entry(&mut self, from: usize, to: usize) -> bool {
        let old_len = self.queue.len();
        if !self.queue.move_entry(from, to) {
            return false;
        }
        self.remap(old_len, |index| {
            Some(queue_index_after_move(from, to, index))
        });
        if let Some(index) = *self.index {
            *self.index = Some(queue_index_after_move(from, to, index));
        }
        true
    }

    pub(super) fn trim_after_current(&mut self) -> bool {
        let Some(current) = self.index.filter(|index| *index < self.queue.len()) else {
            return false;
        };
        let keep = current + 1;
        let old_len = self.queue.len();
        if self.queue.len() <= keep {
            return false;
        }
        self.queue.truncate(keep);
        self.remap(old_len, |index| (index < keep).then_some(index));
        true
    }

    pub(super) fn clear(&mut self) {
        self.queue.clear();
        *self.index = None;
        *self.history = ShuffleHistory::default();
        self.selection.clear_scope(SongScope::Queue);
    }

    fn remap(&mut self, old_len: usize, map: impl Fn(usize) -> Option<usize> + Copy) {
        let new_len = self.queue.len();
        self.history.remap(|index| {
            (index < old_len)
                .then(|| map(index))
                .flatten()
                .filter(|index| *index < new_len)
        });
        self.selection
            .remap_scope(SongScope::Queue, old_len, new_len, map);
    }
}

#[cfg(test)]
mod selection_tests;
#[cfg(test)]
mod tests;
