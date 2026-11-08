//! Resolve visible row events before applying selection or queue operations.

use super::scope::SongScope;
use crate::app::shell::state::AppState;
use crate::app::shell::view_helpers::SongRowClickAction;
use crate::app::NavidromeApp;
use crate::models::QueueEntryId;

#[derive(Clone, Copy)]
enum RowPosition {
    Queue(QueueEntryId),
    List(usize),
}

/// Ordinary lists keep their live scoped projection. Queue events follow the
/// captured occurrence instead of a row index that can refer to another copy.
#[derive(Clone, Copy)]
pub(in crate::app) struct SongRowTarget {
    scope: SongScope,
    position: RowPosition,
    source: Option<[u8; 16]>,
}

impl SongRowTarget {
    pub(in crate::app) fn capture(
        state: &AppState,
        scope: SongScope,
        index: usize,
    ) -> Option<Self> {
        let song = state.songs_for_scope(scope)?.get(index)?;
        let source = song.source.as_ref().map(|source| source.fingerprint());
        let position = if scope == SongScope::Queue {
            RowPosition::Queue(state.queue.entry_id(index)?)
        } else {
            RowPosition::List(index)
        };
        Some(Self {
            scope,
            position,
            source,
        })
    }

    pub(in crate::app) fn resolve(self, state: &AppState) -> Option<(SongScope, usize)> {
        let index = match self.position {
            RowPosition::Queue(entry) => state.queue.index_of(entry)?,
            RowPosition::List(index) => index,
        };
        let song = state.songs_for_scope(self.scope)?.get(index)?;
        if song.source.as_ref().map(|source| source.fingerprint()) != self.source {
            return None;
        }
        Some((self.scope, index))
    }

    fn queue_index(self, state: &AppState) -> Option<usize> {
        match self.position {
            RowPosition::Queue(entry) => state.queue.index_of(entry),
            RowPosition::List(_) => None,
        }
    }

    fn queue_move(self, state: &AppState, delta: isize) -> Option<(usize, usize)> {
        let from = self.queue_index(state)?;
        let to = from.checked_add_signed(delta)?;
        (to < state.queue.len() && from != to).then_some((from, to))
    }
}

impl NavidromeApp {
    pub(in crate::app) fn click_song_row(
        &mut self,
        target: SongRowTarget,
        action: SongRowClickAction,
    ) {
        let Some((scope, index)) = target.resolve(&self.state) else {
            return;
        };
        match action {
            SongRowClickAction::Select => self.select_song_row(scope, index),
            SongRowClickAction::ToggleMulti => self.toggle_song_multi(scope, index),
            SongRowClickAction::RangeMulti => self.range_song_multi(scope, index),
            SongRowClickAction::Play => {
                self.select_song_row(scope, index);
                self.play_song_at_scope(scope, index);
            }
        }
    }

    pub(in crate::app) fn move_queue_row(&mut self, target: SongRowTarget, delta: isize) {
        if let Some((from, to)) = target.queue_move(&self.state, delta) {
            self.move_queue_item(from, to);
        }
    }

    pub(in crate::app) fn remove_queue_row(&mut self, target: SongRowTarget) {
        if let Some(index) = target.queue_index(&self.state) {
            self.remove_from_queue(index);
        }
    }
}

#[cfg(test)]
mod tests;
