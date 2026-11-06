//! Deferred album and artist playback with one outstanding user intent.

use super::library_helpers::sort_album_songs;
use super::library_requests::{LibraryRequest, LibraryRequests};
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PlaybackSource {
    Album,
    Artist,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LibraryPlaybackRequest {
    id: LibraryRequest,
    source: PlaybackSource,
}

#[derive(Default)]
pub(super) struct LibraryPlayback {
    pending: Option<LibraryPlaybackRequest>,
}

impl LibraryPlayback {
    fn begin(
        &mut self,
        requests: &mut LibraryRequests,
        source: PlaybackSource,
    ) -> LibraryPlaybackRequest {
        let request = LibraryPlaybackRequest {
            id: requests.next(),
            source,
        };
        self.pending = Some(request);
        request
    }

    pub(super) fn cancel(&mut self) {
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
    pub(super) fn play_album(&mut self, album: Album) {
        let Some(api) = self.api.clone() else { return };
        self.prepare_manual_queue_change();
        self.ensure_cover(album.cover_art.as_deref(), true);
        let request = self
            .state
            .library_playback
            .begin(&mut self.library_requests, PlaybackSource::Album);
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.album_songs(&album.id).await.map_err(error_message);
            let _ = tx.send(Msg::LibraryPlayback { request, result });
        });
    }

    /// 立即播放艺术家的全部歌曲，保留正在浏览的详情页。
    pub(super) fn play_artist(&mut self, artist: Artist) {
        let Some(api) = self.api.clone() else { return };
        self.prepare_manual_queue_change();
        self.ensure_cover(artist.cover_art.as_deref(), true);
        let request = self
            .state
            .library_playback
            .begin(&mut self.library_requests, PlaybackSource::Artist);
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

    pub(super) fn finish_library_playback(
        &mut self,
        request: LibraryPlaybackRequest,
        result: Result<Vec<Song>, String>,
    ) {
        let Some(result) = self.state.library_playback.complete(request, result) else {
            return;
        };
        match result {
            Ok(songs) if !songs.is_empty() => {
                let covers = songs
                    .iter()
                    .filter_map(|song| song.cover_art.clone())
                    .take(40)
                    .collect();
                self.preload_covers(covers);
                self.replace_queue_and_play(songs, 0, QueueOrigin::Manual);
            }
            Ok(_) => {}
            Err(error) => self.notify_error(error),
        }
    }
}

#[cfg(test)]
mod tests;
