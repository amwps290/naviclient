//! Prepare a playback request before committing its queue and selected track.

use std::time::Duration;

use super::playback_occurrence::Selection;
use super::queue_logic::ShuffleHistory;
use super::session_state::SessionState;
use super::song_scope::SongScope;
use super::state::AppState;
use crate::api::OpenSubsonicProvider;
use crate::models::{PlaybackQueue, QueueEntry, QueueEntryId, Song, TranscodingQuality};
use crate::playback_timeline::song_duration_hint;

enum QueueChange {
    Keep,
    Replace(PlaybackQueue),
    Append {
        entries: Vec<QueueEntry>,
        before: Vec<QueueEntryId>,
    },
}

pub(super) struct StartTarget {
    index: usize,
    entry: QueueEntryId,
    song: Song,
    change: QueueChange,
}

impl StartTarget {
    pub(super) fn existing(queue: &PlaybackQueue, index: usize) -> Option<Self> {
        Some(Self {
            index,
            entry: queue.entry_id(index)?,
            song: queue.song(index)?.clone(),
            change: QueueChange::Keep,
        })
    }

    pub(super) fn replacement(queue: PlaybackQueue, index: usize) -> Option<Self> {
        let mut target = Self::existing(&queue, index)?;
        target.change = QueueChange::Replace(queue);
        Some(target)
    }

    pub(super) fn append(queue: &PlaybackQueue, entries: Vec<QueueEntry>) -> Option<Self> {
        let first = entries.first()?;
        Some(Self {
            index: queue.len(),
            entry: first.id(),
            song: first.song.clone(),
            change: QueueChange::Append {
                before: queue.entries().iter().map(QueueEntry::id).collect(),
                entries,
            },
        })
    }

    fn matches(&self, queue: &PlaybackQueue) -> bool {
        match &self.change {
            QueueChange::Keep => queue.entry_id(self.index) == Some(self.entry),
            QueueChange::Replace(_) => true,
            QueueChange::Append { before, .. } => queue
                .entries()
                .iter()
                .map(QueueEntry::id)
                .eq(before.iter().copied()),
        }
    }
}

pub(super) struct PlaybackRequest {
    pub(super) url: String,
    pub(super) cache_key: String,
    pub(super) duration: Option<Duration>,
    pub(super) quality: TranscodingQuality,
    pub(super) position: Duration,
    pub(super) gain: f32,
}

impl PlaybackRequest {
    pub(super) fn prepare(
        api: &OpenSubsonicProvider,
        song: &Song,
        configured: TranscodingQuality,
    ) -> anyhow::Result<Self> {
        Self::prepare_with(api, song, configured, |api, song, quality| {
            api.stream_url(&song.id, quality.max_bit_rate())
        })
    }

    fn prepare_with(
        api: &OpenSubsonicProvider,
        song: &Song,
        configured: TranscodingQuality,
        url: impl FnOnce(&OpenSubsonicProvider, &Song, TranscodingQuality) -> anyhow::Result<String>,
    ) -> anyhow::Result<Self> {
        let quality = TranscodingQuality::effective_for(
            configured,
            song.suffix.as_deref(),
            song.content_type.as_deref(),
        );
        Ok(Self {
            url: url(api, song, quality)?,
            cache_key: format!(
                "{}:{}:profile={}",
                api.base_url(),
                song.id,
                quality.cache_profile()
            ),
            duration: song_duration_hint(song),
            quality,
            position: Duration::ZERO,
            gain: 1.0,
        })
    }
}

#[derive(Debug)]
pub(super) enum StartError {
    NoServer,
    Request(anyhow::Error),
    Stale,
    OutputUnavailable,
}

pub(super) struct PreparedStart {
    target: StartTarget,
    pub(super) request: PlaybackRequest,
    selection: Selection,
    history: Option<ShuffleHistory>,
}

pub(super) struct StartEffects {
    pub(super) song: Song,
    pub(super) position: Duration,
    pub(super) finished_listening: bool,
}

impl PreparedStart {
    pub(super) fn prepare(
        target: StartTarget,
        api: Option<&OpenSubsonicProvider>,
        configured: TranscodingQuality,
        position: Duration,
        selection: Selection,
    ) -> Result<Self, StartError> {
        Self::prepare_with(
            target,
            api,
            configured,
            position,
            selection,
            PlaybackRequest::prepare,
        )
    }

    fn prepare_with(
        target: StartTarget,
        api: Option<&OpenSubsonicProvider>,
        configured: TranscodingQuality,
        position: Duration,
        selection: Selection,
        prepare: impl FnOnce(
            &OpenSubsonicProvider,
            &Song,
            TranscodingQuality,
        ) -> anyhow::Result<PlaybackRequest>,
    ) -> Result<Self, StartError> {
        let api = api.ok_or(StartError::NoServer)?;
        let mut request = prepare(api, &target.song, configured).map_err(StartError::Request)?;
        request.position = request
            .duration
            .map_or(position, |limit| position.min(limit));
        Ok(Self {
            target,
            request,
            selection,
            history: None,
        })
    }

    pub(super) fn song(&self) -> &Song {
        &self.target.song
    }

    pub(super) fn entry(&self) -> QueueEntryId {
        self.target.entry
    }

    pub(super) fn new_occurrence(&self) -> bool {
        self.selection == Selection::New
    }

    pub(super) fn with_history(mut self, history: ShuffleHistory) -> Self {
        self.history = Some(history);
        self
    }

    pub(super) fn commit(
        self,
        state: &mut AppState,
        session: &mut SessionState,
        send: impl FnOnce(&PlaybackRequest) -> bool,
    ) -> Result<StartEffects, StartError> {
        if !self.target.matches(&state.queue) {
            return Err(StartError::Stale);
        }
        if !send(&self.request) {
            return Err(StartError::OutputUnavailable);
        }
        // Everything after accepting the command is an infallible commit of
        // owned data. The worker and UI now describe the same requested track.
        match self.target.change {
            QueueChange::Keep => {}
            QueueChange::Replace(queue) => {
                state.song_selection.clear_scope(SongScope::Queue);
                state.queue = queue;
            }
            QueueChange::Append { entries, .. } => {
                state.edit_queue().append_entries(entries);
            }
        }
        if let Some(history) = self.history {
            state.shuffle_history = history;
        }
        let finished_listening = select_state(
            state,
            self.target.index,
            &self.target.song,
            self.request.quality,
            self.selection,
        );
        session.clear_position();
        Ok(StartEffects {
            song: self.target.song,
            position: self.request.position,
            finished_listening,
        })
    }
}

pub(super) fn select_state(
    state: &mut AppState,
    index: usize,
    song: &Song,
    quality: TranscodingQuality,
    selection: Selection,
) -> bool {
    let Some(entry) = state.queue.entry_id(index) else {
        return false;
    };
    let finished_listening =
        state.occurrence.select(entry, &song.id, selection) && state.listening.finish(false);
    state.queue_index = Some(index);
    state.now_playing = Some(song.clone());
    state.now_playing_quality = Some(quality);
    finished_listening
}

#[cfg(test)]
mod tests;
