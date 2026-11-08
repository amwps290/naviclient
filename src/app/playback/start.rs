//! Prepare a playback request before committing its queue and selected track.

use std::collections::HashSet;
use std::time::Duration;

use super::occurrence::Selection;
use super::session_state::SessionState;
use crate::app::queue::logic::ShuffleHistory;
use crate::app::shell::state::AppState;
use crate::app::songs::scope::SongScope;
use crate::models::{PlaybackQueue, QueueEntry, QueueEntryId, Song, TranscodingQuality};
use crate::source::MusicSource;
use crate::source::{HttpResource, PlaybackResource, SourceId};

enum QueueChange {
    Keep,
    Replace(PlaybackQueue),
    Append {
        entries: Vec<QueueEntry>,
        before: Vec<QueueEntryId>,
        owners: HashSet<SourceId>,
    },
}

pub(in crate::app) struct StartTarget {
    index: usize,
    entry: QueueEntryId,
    song: Song,
    change: QueueChange,
}

impl StartTarget {
    pub(in crate::app) fn source<'a>(
        &self,
        browsing: Option<&'a MusicSource>,
        playback: Option<&'a MusicSource>,
    ) -> Option<&'a MusicSource> {
        match self.change {
            QueueChange::Keep | QueueChange::Append { .. } => playback,
            QueueChange::Replace(_) => browsing
                .filter(|api| self.song.belongs_to(api.id()))
                .or_else(|| playback.filter(|api| self.song.belongs_to(api.id()))),
        }
    }

    pub(in crate::app) fn existing(queue: &PlaybackQueue, index: usize) -> Option<Self> {
        Some(Self {
            index,
            entry: queue.entry_id(index)?,
            song: queue.song(index)?.clone(),
            change: QueueChange::Keep,
        })
    }

    pub(in crate::app) fn replacement(queue: PlaybackQueue, index: usize) -> Option<Self> {
        let mut target = Self::existing(&queue, index)?;
        target.change = QueueChange::Replace(queue);
        Some(target)
    }

    pub(in crate::app) fn append(queue: &PlaybackQueue, entries: Vec<QueueEntry>) -> Option<Self> {
        let first = entries.first()?;
        Some(Self {
            index: queue.len(),
            entry: first.id(),
            song: first.song.clone(),
            change: QueueChange::Append {
                before: queue.entries().iter().map(QueueEntry::id).collect(),
                owners: queue
                    .songs()
                    .filter_map(|song| song.source.clone())
                    .collect(),
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

    fn bind_source(&mut self, source: &SourceId) -> bool {
        if !self.song.belongs_to(source) {
            return false;
        }
        let valid = match &mut self.change {
            QueueChange::Keep => true,
            QueueChange::Replace(queue) => queue.bind_source(source),
            QueueChange::Append {
                entries, owners, ..
            } => {
                if owners.iter().any(|owner| owner != source)
                    || entries.iter().any(|entry| !entry.song.belongs_to(source))
                {
                    return false;
                }
                for entry in entries {
                    entry.song.source = Some(source.clone());
                }
                true
            }
        };
        if valid {
            self.song.source = Some(source.clone());
        }
        valid
    }
}

pub(in crate::app) struct PlaybackRequest {
    pub(in crate::app) url: HttpResource,
    pub(in crate::app) cache_key: String,
    pub(in crate::app) duration: Option<Duration>,
    pub(in crate::app) quality: TranscodingQuality,
    pub(in crate::app) audio_quality: Option<crate::source::AudioQuality>,
    pub(in crate::app) position: Duration,
    pub(in crate::app) gain: f32,
}

impl PlaybackRequest {
    pub(in crate::app) fn from_resource(
        resource: PlaybackResource,
        song: &Song,
        configured: TranscodingQuality,
    ) -> anyhow::Result<Self> {
        if song.media_key().as_ref() != Some(&resource.media) {
            anyhow::bail!("resolved audio belongs to another media item");
        }
        Ok(Self {
            cache_key: resource.cache_key(),
            duration: resource.duration,
            url: resource.http.with_backups(resource.backups),
            audio_quality: (resource.media.source.provider
                == crate::source::ProviderKind::Bilibili)
                .then_some(resource.quality),
            quality: if resource.media.source.provider == crate::source::ProviderKind::Bilibili {
                TranscodingQuality::Original
            } else {
                TranscodingQuality::effective_for(
                    configured,
                    song.suffix.as_deref(),
                    song.content_type.as_deref(),
                )
            },
            position: Duration::ZERO,
            gain: 1.0,
        })
    }

    #[cfg(test)]
    pub(in crate::app) fn prepare(
        api: &MusicSource,
        song: &Song,
        configured: TranscodingQuality,
    ) -> anyhow::Result<Self> {
        let resource = api.resolve_immediate(song, configured)?;
        let mut song = song.clone();
        song.source = Some(api.id().clone());
        Self::from_resource(resource, &song, configured)
    }
}

#[derive(Debug)]
pub(in crate::app) enum StartError {
    NoServer,
    Request(anyhow::Error),
    Stale,
    OutputUnavailable,
}

pub(in crate::app) struct PreparedStart {
    target: StartTarget,
    pub(in crate::app) request: PlaybackRequest,
    selection: Selection,
    history: Option<ShuffleHistory>,
    source: MusicSource,
}

pub(in crate::app) struct StartEffects {
    pub(in crate::app) song: Song,
    pub(in crate::app) position: Duration,
    pub(in crate::app) finished_listening: bool,
}

pub(in crate::app) struct UnresolvedStart {
    target: StartTarget,
    pub(in crate::app) source: MusicSource,
    configured: TranscodingQuality,
    position: Duration,
    selection: Selection,
    history: Option<ShuffleHistory>,
    remaining_skips: u8,
}

impl UnresolvedStart {
    pub(in crate::app) fn new(
        mut target: StartTarget,
        source: Option<&MusicSource>,
        configured: TranscodingQuality,
        position: Duration,
        selection: Selection,
    ) -> Result<Self, StartError> {
        let source = source.ok_or(StartError::NoServer)?;
        if !target.bind_source(source.id()) {
            return Err(StartError::Request(anyhow::anyhow!(
                "queue contains media from another source"
            )));
        }
        Ok(Self {
            target,
            source: source.clone(),
            configured,
            position,
            selection,
            history: None,
            remaining_skips: 20,
        })
    }

    pub(in crate::app) fn song(&self) -> &Song {
        &self.target.song
    }

    pub(in crate::app) fn with_history(mut self, history: ShuffleHistory) -> Self {
        self.history = Some(history);
        self
    }

    pub(in crate::app) fn try_resolve(&self) -> Option<anyhow::Result<PlaybackResource>> {
        self.source
            .try_resolve_immediate(self.song(), self.configured)
    }

    pub(in crate::app) fn configured(&self) -> TranscodingQuality {
        self.configured
    }

    /// Skip only authoritative per-item failures during new playback. Session
    /// renewal retains its selected item, and the old source is never committed
    /// merely because one item in a replacement queue was unavailable.
    pub(in crate::app) fn skip_unavailable(mut self, current: &PlaybackQueue) -> Option<Self> {
        if self.source.id().provider != crate::source::ProviderKind::Bilibili
            || self.selection != Selection::New
            || self.remaining_skips == 0
            || !self.target.matches(current)
        {
            return None;
        }
        let queue = match &self.target.change {
            QueueChange::Keep => current,
            QueueChange::Replace(queue) => queue,
            QueueChange::Append { .. } => return None,
        };
        let index = self.target.index.checked_add(1)?;
        let entry = queue.entry_id(index)?;
        let song = queue.song(index)?.clone();
        if !song.belongs_to(self.source.id()) {
            return None;
        }
        let skipped = self.target.index;
        self.target.index = index;
        self.target.entry = entry;
        self.target.song = song;
        self.target.bind_source(self.source.id());
        self.position = Duration::ZERO;
        self.remaining_skips -= 1;
        if let Some(history) = &mut self.history {
            // The failed candidate was only staged for playback. Previous
            // must return to a track that actually played, not this candidate.
            if history.played.last() == Some(&skipped) {
                history.played.pop();
            }
            history.advance(index);
        }
        Some(self)
    }

    pub(in crate::app) fn finish(
        self,
        resource: PlaybackResource,
    ) -> Result<PreparedStart, StartError> {
        let mut request = PlaybackRequest::from_resource(resource, self.song(), self.configured)
            .map_err(StartError::Request)?;
        request.position = request
            .duration
            .map_or(self.position, |limit| self.position.min(limit));
        Ok(PreparedStart {
            target: self.target,
            request,
            selection: self.selection,
            history: self.history,
            source: self.source,
        })
    }
}

impl PreparedStart {
    #[cfg(test)]
    pub(in crate::app) fn prepare(
        target: StartTarget,
        api: Option<&MusicSource>,
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

    #[cfg(test)]
    fn prepare_with(
        mut target: StartTarget,
        api: Option<&MusicSource>,
        configured: TranscodingQuality,
        position: Duration,
        selection: Selection,
        prepare: impl FnOnce(&MusicSource, &Song, TranscodingQuality) -> anyhow::Result<PlaybackRequest>,
    ) -> Result<Self, StartError> {
        let api = api.ok_or(StartError::NoServer)?;
        if !target.bind_source(api.id()) {
            return Err(StartError::Request(anyhow::anyhow!(
                "queue contains media from another source"
            )));
        }
        let mut request = prepare(api, &target.song, configured).map_err(StartError::Request)?;
        request.position = request
            .duration
            .map_or(position, |limit| position.min(limit));
        Ok(Self {
            target,
            request,
            selection,
            history: None,
            source: api.clone(),
        })
    }

    pub(in crate::app) fn song(&self) -> &Song {
        &self.target.song
    }

    pub(in crate::app) fn source(&self) -> &MusicSource {
        &self.source
    }

    pub(in crate::app) fn entry(&self) -> QueueEntryId {
        self.target.entry
    }

    pub(in crate::app) fn new_occurrence(&self) -> bool {
        self.selection == Selection::New
    }

    #[cfg(test)]
    pub(in crate::app) fn with_history(mut self, history: ShuffleHistory) -> Self {
        self.history = Some(history);
        self
    }

    pub(in crate::app) fn commit(
        self,
        state: &mut AppState,
        session: &mut SessionState,
        send: impl FnOnce(&PlaybackRequest) -> bool,
    ) -> Result<StartEffects, StartError> {
        if !self.target.matches(&state.queue) {
            return Err(StartError::Stale);
        }
        if !matches!(self.target.change, QueueChange::Replace(_))
            && state
                .queue
                .songs()
                .any(|song| !song.belongs_to(self.source.id()))
        {
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
        state.now_playing_audio_quality = self.request.audio_quality;
        session.clear_position();
        Ok(StartEffects {
            song: self.target.song,
            position: self.request.position,
            finished_listening,
        })
    }
}

pub(in crate::app) fn select_state(
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
    state.now_playing_audio_quality = None;
    finished_listening
}

#[cfg(test)]
mod tests;
