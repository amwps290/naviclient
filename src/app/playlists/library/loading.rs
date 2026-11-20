//! Playlist preview acceptance, completion, and bounded stream ownership.

use std::time::Instant;
use tokio::sync::mpsc::{error::TryRecvError, Receiver};

#[cfg(test)]
use super::PlaylistDetails;
use super::{LibraryRequest, PlaylistLibrary};
use crate::playlist::loading::{
    PlaylistFailureKind, PlaylistLoadError, PlaylistProgress, PlaylistUpdate, PLAYLIST_BATCH_SIZE,
};
use crate::source::{ProviderKind, SourceId};

struct DetailStream {
    request: LibraryRequest,
    source: SourceId,
    updates: Receiver<PlaylistUpdate>,
    started: Instant,
}

#[cfg(test)]
mod tests;

#[derive(Default)]
pub(super) struct PlaylistDetailLoad {
    stream: Option<DetailStream>,
    snapshot: Option<LibraryRequest>,
    complete: bool,
    replacing: bool,
    metadata_received: bool,
    metadata_cover: Option<String>,
    received_songs: usize,
    duration: u64,
    progress: PlaylistProgress,
}

impl PlaylistDetailLoad {
    pub(super) fn begin(&mut self) {
        self.stream = None;
        self.complete = false;
        self.replacing = true;
        self.metadata_received = false;
        self.metadata_cover = None;
        self.received_songs = 0;
        self.duration = 0;
        self.progress = PlaylistProgress::default();
    }

    pub(super) fn cancel(&mut self) {
        self.stream = None;
    }

    #[cfg(test)]
    pub(super) fn accept_complete(&mut self, request: LibraryRequest, details: &PlaylistDetails) {
        self.stream = None;
        self.snapshot = Some(request);
        self.complete = true;
        self.replacing = false;
        self.received_songs = details.songs.len();
        self.progress = PlaylistProgress {
            songs: details.songs.len(),
            unavailable_count: details.unavailable_count,
            ..Default::default()
        };
    }
}

pub(in crate::app) enum PlaylistDetailChange {
    Updated { reset: bool },
    Complete { reset: bool, unavailable: usize },
    Failed { reset: bool, message: String },
}

impl PlaylistLibrary {
    pub(in crate::app) fn detail_ready(&self) -> bool {
        self.detail.complete && !self.detail_load.loading()
    }

    pub(in crate::app) fn snapshot(&self) -> Option<LibraryRequest> {
        self.detail.snapshot
    }

    pub(in crate::app) fn unavailable_count(&self) -> usize {
        self.selected
            .as_ref()
            .map_or(0, |details| details.unavailable_count)
    }

    pub(in crate::app) fn attach_detail_stream(
        &mut self,
        request: LibraryRequest,
        source: SourceId,
        updates: Receiver<PlaylistUpdate>,
        task: tokio::task::AbortHandle,
    ) {
        if !self.detail_load.pending(request) {
            task.abort();
            return;
        }
        self.detail.stream = Some(DetailStream {
            request,
            source,
            updates,
            started: Instant::now(),
        });
        self.detail_task = Some(task);
    }

    pub(in crate::app) fn next_detail_update(
        &mut self,
    ) -> Option<(LibraryRequest, PlaylistUpdate)> {
        let stream = self.detail.stream.as_mut()?;
        let request = stream.request;
        match stream.updates.try_recv() {
            Ok(update) => Some((request, update)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.detail.stream = None;
                Some((
                    request,
                    PlaylistUpdate::Failed(PlaylistLoadError::interrupted()),
                ))
            }
        }
    }

    pub(in crate::app) fn apply_detail_update(
        &mut self,
        request: LibraryRequest,
        update: PlaylistUpdate,
    ) -> Option<PlaylistDetailChange> {
        if !self.detail_load.pending(request) {
            return None;
        }
        Some(match update {
            PlaylistUpdate::Metadata(mut playlist) => {
                let selected = self.selected.as_mut()?;
                if playlist.id != selected.playlist.id {
                    return Some(self.fail_detail(request, PlaylistLoadError::invalid_response()));
                }
                if playlist.name.is_empty() {
                    playlist.name.clone_from(&selected.playlist.name);
                }
                if playlist.song_count.is_none() {
                    playlist.song_count = selected.playlist.song_count;
                }
                if playlist.duration.is_none() {
                    playlist.duration = selected.playlist.duration;
                }
                if playlist.owner.is_none() {
                    playlist.owner.clone_from(&selected.playlist.owner);
                }
                if playlist.public.is_none() {
                    playlist.public = selected.playlist.public;
                }
                self.detail.metadata_cover = playlist.cover_art.clone();
                if playlist.cover_art.is_none() {
                    playlist.cover_art = selected.playlist.cover_art.clone();
                }
                selected.playlist = playlist;
                self.detail.metadata_received = true;
                PlaylistDetailChange::Updated { reset: false }
            }
            PlaylistUpdate::Batch { songs, progress } => {
                let valid_source = self.detail.stream.as_ref().is_none_or(|stream| {
                    songs
                        .iter()
                        .all(|song| song.source.as_ref() == Some(&stream.source))
                });
                if !self.detail.metadata_received
                    || songs.is_empty()
                    || songs.len() > PLAYLIST_BATCH_SIZE
                    || progress.songs != self.detail.received_songs.saturating_add(songs.len())
                    || !valid_source
                {
                    return Some(self.fail_detail(request, PlaylistLoadError::invalid_response()));
                }
                let reset = self.replace_preview(request);
                let selected = self.selected.as_mut()?;
                if selected.playlist.cover_art.is_none() {
                    selected.playlist.cover_art =
                        songs.iter().find_map(|song| song.cover_art.clone());
                }
                self.detail.duration = self.detail.duration.saturating_add(
                    songs
                        .iter()
                        .map(|song| song.duration.unwrap_or_default().max(0) as u64)
                        .sum(),
                );
                selected.songs.extend(songs);
                selected.unavailable_count = progress.unavailable_count;
                self.detail.received_songs = progress.songs;
                self.detail.progress = progress;
                if reset {
                    if let Some(stream) = &self.detail.stream {
                        log::debug!(
                            "playlist first batch applied; elapsed_ms={} songs={}",
                            stream.started.elapsed().as_millis(),
                            self.detail.received_songs,
                        );
                    }
                }
                PlaylistDetailChange::Updated { reset }
            }
            PlaylistUpdate::Progress(progress) => {
                if progress.songs != self.detail.received_songs {
                    return Some(self.fail_detail(request, PlaylistLoadError::invalid_response()));
                }
                self.detail.progress = progress;
                self.selected.as_mut()?.unavailable_count = progress.unavailable_count;
                PlaylistDetailChange::Updated { reset: false }
            }
            PlaylistUpdate::Complete(progress) => {
                if !self.detail.metadata_received || progress.songs != self.detail.received_songs {
                    return Some(self.fail_detail(request, PlaylistLoadError::invalid_response()));
                }
                let bilibili = self
                    .detail
                    .stream
                    .as_ref()
                    .is_some_and(|stream| stream.source.provider == ProviderKind::Bilibili);
                let reset = self.replace_preview(request);
                let selected = self.selected.as_mut()?;
                selected.unavailable_count = progress.unavailable_count;
                if bilibili {
                    selected.playlist.song_count = i32::try_from(progress.songs).ok();
                    selected.playlist.duration = i32::try_from(self.detail.duration).ok();
                }
                if let Some(item) = self
                    .items
                    .iter_mut()
                    .find(|item| item.id == selected.playlist.id)
                {
                    item.cover_art = selected.playlist.cover_art.clone();
                }
                self.detail.progress = progress;
                self.detail.complete = true;
                self.detail_load.complete(request, Ok(()));
                self.finish_detail_stream();
                PlaylistDetailChange::Complete {
                    reset,
                    unavailable: progress.unavailable_count,
                }
            }
            PlaylistUpdate::Failed(error) => self.fail_detail(request, error),
        })
    }

    fn replace_preview(&mut self, request: LibraryRequest) -> bool {
        if !self.detail.replacing {
            return false;
        }
        if let Some(selected) = &mut self.selected {
            selected.songs.clear();
            selected.unavailable_count = 0;
            // Keep the old artwork while retrying, then derive fallback artwork
            // from the new prefix instead of carrying the old first song over.
            selected.playlist.cover_art = self.detail.metadata_cover.clone();
        }
        self.detail.snapshot = Some(request);
        self.detail.replacing = false;
        true
    }

    fn fail_detail(
        &mut self,
        request: LibraryRequest,
        error: PlaylistLoadError,
    ) -> PlaylistDetailChange {
        let reset = error.kind == PlaylistFailureKind::Authorization;
        if reset {
            if let Some(selected) = &mut self.selected {
                selected.songs.clear();
                selected.unavailable_count = 0;
            }
            self.detail.snapshot = None;
        }
        self.detail.complete = false;
        self.detail_load
            .complete::<()>(request, Err(error.message.clone()));
        self.finish_detail_stream();
        PlaylistDetailChange::Failed {
            reset,
            message: error.message,
        }
    }

    fn finish_detail_stream(&mut self) {
        self.detail.stream = None;
        if let Some(task) = self.detail_task.take() {
            task.abort();
        }
    }
}
