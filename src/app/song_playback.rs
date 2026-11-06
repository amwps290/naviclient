//! Scoped playback plans and dispatch for row, detail and multi-selection queue actions.

use super::queue_logic::ShuffleHistory;
use super::song_list::SongList;
use super::song_scope::SongScope;
use super::song_selection::SongSelection;
use super::state::AppState;
use super::NavidromeApp;
use crate::models::{QueueOrigin, Song};

#[derive(Debug)]
pub(super) enum SongPlayback {
    /// Keep all existing queue occurrences and their origins.
    QueueIndex(usize),
    /// The selected songs are owned and consumed once by queue replacement.
    Replace {
        songs: Vec<Song>,
        index: usize,
        origin: QueueOrigin,
    },
}

impl SongPlayback {
    fn row(scope: SongScope, songs: SongList<'_>, index: usize) -> Option<Self> {
        let song = songs.get(index)?;
        Some(match scope {
            SongScope::Queue => Self::QueueIndex(index),
            SongScope::History => Self::Replace {
                songs: vec![song.clone()],
                index: 0,
                origin: scope.queue_origin(),
            },
            _ => Self::Replace {
                songs: songs.to_vec(),
                index,
                origin: scope.queue_origin(),
            },
        })
    }

    fn selected(scope: SongScope, songs: SongList<'_>, selection: &SongSelection) -> Option<Self> {
        let mut indices = selection.selected_indices(scope, songs.len());
        let first = indices.next()?;
        Some(if scope == SongScope::Queue {
            Self::QueueIndex(first)
        } else {
            Self::Replace {
                songs: std::iter::once(first)
                    .chain(indices)
                    .map(|index| songs[index].clone())
                    .collect(),
                index: 0,
                origin: scope.queue_origin(),
            }
        })
    }
}

impl AppState {
    pub(super) fn song_playback_at(&self, scope: SongScope, index: usize) -> Option<SongPlayback> {
        SongPlayback::row(scope, self.songs_for_scope(scope)?, index)
    }

    pub(super) fn selected_playback(&self, scope: SongScope) -> Option<SongPlayback> {
        SongPlayback::selected(scope, self.songs_for_scope(scope)?, &self.song_selection)
    }
}

impl NavidromeApp {
    /// Append the current list with its scope's origin, without starting playback.
    pub(super) fn append_songs_at_scope(&mut self, scope: SongScope) {
        if let Some(songs) = self.state.songs_for_scope(scope) {
            let songs = songs.to_vec();
            self.append_all_with_origin(songs, scope.queue_origin());
        }
    }

    /// Insert selected songs as one block, preserving row order and remapping
    /// shuffle history once.
    pub(super) fn selected_play_next(&mut self, scope: SongScope) {
        let songs = self.selected_songs(scope);
        self.insert_next_many(songs);
    }

    /// Selected additions use Manual origin, including playlist selections.
    pub(super) fn selected_to_queue(&mut self, scope: SongScope) {
        let songs = self.selected_songs(scope);
        self.append_all(songs);
    }

    /// Ordinary lists replace the queue; History row playback uses just that
    /// event's song. Queue rows only select the existing occurrence.
    pub(super) fn play_song_at_scope(&mut self, scope: SongScope, index: usize) {
        if let Some(playback) = self.state.song_playback_at(scope, index) {
            self.apply_song_playback(playback);
        }
    }

    /// Multi-selection preserves row order. Queue selection starts its first
    /// selected occurrence without replacing the queue with a selected subset.
    pub(super) fn play_selected_songs(&mut self, scope: SongScope) {
        if let Some(playback) = self.state.selected_playback(scope) {
            self.apply_song_playback(playback);
        }
    }

    fn apply_song_playback(&mut self, playback: SongPlayback) {
        match playback {
            SongPlayback::QueueIndex(index) => {
                let mut history = ShuffleHistory::default();
                history.start(index);
                self.prepare_manual_queue_change();
                if self.play_queue_index_with_history(index, history) {
                    self.save_session();
                }
            }
            SongPlayback::Replace {
                songs,
                index,
                origin,
            } => {
                self.replace_queue_and_play(songs, index, origin);
            }
        }
    }
}

#[cfg(test)]
mod tests;
