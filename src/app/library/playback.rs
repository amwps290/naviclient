//! Deferred album and artist playback with one outstanding user intent.

use super::helpers::sort_album_songs;
use super::requests::{LibraryRequest, LibraryRequests};
use crate::app::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaybackSource {
    Album,
    Artist,
    Playlist,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum LibraryQueueAction {
    ReplaceAndPlay,
    PlayNext,
    Append,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LibraryPlaybackRequest {
    id: LibraryRequest,
    source: PlaybackSource,
    action: LibraryQueueAction,
}

#[derive(Default)]
pub(in crate::app) struct LibraryPlayback {
    pending: Option<LibraryPlaybackRequest>,
}

impl LibraryPlayback {
    fn begin(
        &mut self,
        requests: &mut LibraryRequests,
        source: PlaybackSource,
        action: LibraryQueueAction,
    ) -> LibraryPlaybackRequest {
        let request = LibraryPlaybackRequest {
            id: requests.next(),
            source,
            action,
        };
        self.pending = Some(request);
        request
    }

    pub(in crate::app) fn cancel(&mut self) {
        self.pending = None;
    }

    fn complete(
        &mut self,
        request: LibraryPlaybackRequest,
        result: Result<Vec<Song>, String>,
    ) -> Option<Result<Vec<Song>, String>> {
        if self.pending != Some(request) {
            return None;
        }
        self.pending = None;
        Some(result.map(|mut songs| {
            if request.source == PlaybackSource::Album {
                sort_album_songs(&mut songs);
            }
            songs
        }))
    }
}

impl NavidromeApp {
    /// 立即播放整张专辑，保留正在浏览的详情页。
    pub(in crate::app) fn play_album(&mut self, album: Album) {
        self.queue_album(album, LibraryQueueAction::ReplaceAndPlay);
    }

    pub(in crate::app) fn queue_album(&mut self, album: Album, action: LibraryQueueAction) {
        let Some(api) = self.api.clone() else { return };
        if action == LibraryQueueAction::ReplaceAndPlay {
            self.prepare_manual_queue_change();
        }
        self.ensure_cover(album.cover_art.as_deref(), true);
        let request = self.state.library_playback.begin(
            &mut self.library_requests,
            PlaybackSource::Album,
            action,
        );
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.album_songs(&album.id).await.map_err(error_message);
            let _ = tx.send(Msg::LibraryPlayback { request, result });
        });
    }

    pub(in crate::app) fn queue_playlist(
        &mut self,
        playlist: Playlist,
        action: LibraryQueueAction,
    ) {
        let Some(api) = self.api.clone() else { return };
        if action == LibraryQueueAction::ReplaceAndPlay {
            self.prepare_manual_queue_change();
        }
        self.ensure_cover(playlist.cover_art.as_deref(), true);
        let request = self.state.library_playback.begin(
            &mut self.library_requests,
            PlaybackSource::Playlist,
            action,
        );
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api
                .playlist(&playlist.id)
                .await
                .map(|details| details.songs)
                .map_err(error_message);
            let _ = tx.send(Msg::LibraryPlayback { request, result });
        });
    }

    /// 立即播放艺术家的全部歌曲，保留正在浏览的详情页。
    pub(in crate::app) fn play_artist(&mut self, artist: Artist) {
        let Some(api) = self.api.clone() else { return };
        self.prepare_manual_queue_change();
        self.ensure_cover(artist.cover_art.as_deref(), true);
        let request = self.state.library_playback.begin(
            &mut self.library_requests,
            PlaybackSource::Artist,
            LibraryQueueAction::ReplaceAndPlay,
        );
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = async {
                let albums = api.artist_albums(&artist.id).await?;
                let mut songs = Vec::new();
                for album in albums {
                    songs.extend(api.album_songs(&album.id).await?);
                }
                Ok::<_, anyhow::Error>(songs)
            }
            .await
            .map_err(error_message);
            let _ = tx.send(Msg::LibraryPlayback { request, result });
        });
    }

    pub(in crate::app) fn finish_library_playback(
        &mut self,
        request: LibraryPlaybackRequest,
        result: Result<Vec<Song>, String>,
    ) {
        let Some(result) = self.state.library_playback.complete(request, result) else {
            return;
        };
        let origin = if request.source == PlaybackSource::Playlist {
            QueueOrigin::Playlist
        } else {
            QueueOrigin::Manual
        };
        match result {
            Ok(songs) if !songs.is_empty() => {
                let covers = songs
                    .iter()
                    .filter_map(|song| song.cover_art.clone())
                    .take(40)
                    .collect();
                self.preload_covers(covers);
                match request.action {
                    LibraryQueueAction::ReplaceAndPlay => {
                        self.replace_queue_and_play(songs, 0, origin);
                    }
                    LibraryQueueAction::PlayNext => {
                        self.insert_next_many(songs);
                    }
                    LibraryQueueAction::Append => {
                        self.append_all_with_origin(songs, origin);
                    }
                }
            }
            Ok(_) => {}
            Err(error) => self.notify_error(error),
        }
    }
}

#[cfg(test)]
mod tests;
