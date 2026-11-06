//! Selected artist, independent album/popular-song loads, and detail actions.

use super::requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use crate::app::prelude::*;

#[derive(Default)]
pub(in crate::app) struct ArtistDetails {
    artist: Option<Artist>,
    albums: Vec<Album>,
    top_songs: Vec<Song>,
    albums_load: LibraryLoad,
    top_songs_load: LibraryLoad,
}

impl ArtistDetails {
    pub(in crate::app) fn artist(&self) -> Option<&Artist> {
        self.artist.as_ref()
    }

    pub(in crate::app) fn albums(&self) -> &[Album] {
        &self.albums
    }

    pub(in crate::app) fn albums_loading(&self) -> bool {
        self.albums_load.loading()
    }

    pub(in crate::app) fn albums_error(&self) -> Option<&str> {
        self.albums_load.error()
    }

    pub(in crate::app) fn top_songs(&self) -> &[Song] {
        &self.top_songs
    }

    pub(in crate::app) fn top_songs_loading(&self) -> bool {
        self.top_songs_load.loading()
    }

    pub(in crate::app) fn top_songs_error(&self) -> Option<&str> {
        self.top_songs_load.error()
    }

    pub(in crate::app) fn select(&mut self, artist: Artist) {
        *self = Self {
            artist: Some(artist),
            ..Default::default()
        };
    }

    pub(in crate::app) fn begin_albums(
        &mut self,
        requests: &mut LibraryRequests,
    ) -> Option<LibraryRequest> {
        self.artist.as_ref()?;
        Some(self.albums_load.begin(requests))
    }

    pub(in crate::app) fn begin_top_songs(
        &mut self,
        requests: &mut LibraryRequests,
    ) -> Option<LibraryRequest> {
        self.artist.as_ref()?;
        Some(self.top_songs_load.begin(requests))
    }

    pub(in crate::app) fn complete_albums(
        &mut self,
        request: LibraryRequest,
        result: Result<Vec<Album>, String>,
    ) -> Option<Result<&[Album], &str>> {
        Some(match self.albums_load.complete(request, result)? {
            Ok(albums) => {
                self.albums = albums;
                Ok(&self.albums)
            }
            Err(error) => Err(error),
        })
    }

    pub(in crate::app) fn complete_top_songs(
        &mut self,
        request: LibraryRequest,
        result: Result<Vec<Song>, String>,
    ) -> Option<Result<&[Song], &str>> {
        Some(match self.top_songs_load.complete(request, result)? {
            Ok(songs) => {
                self.top_songs = songs;
                Ok(&self.top_songs)
            }
            Err(error) => {
                self.top_songs.clear();
                Err(error)
            }
        })
    }
}

impl NavidromeApp {
    pub(in crate::app) fn open_artist(&mut self, artist: Artist) {
        self.clear_song_multi();
        self.ensure_cover(artist.cover_art.as_deref(), true);
        self.state.artist_details.select(artist);
        self.state.view = View::ArtistDetail;
        self.load_artist_albums();
        self.load_artist_top_songs();
    }

    pub(in crate::app) fn load_artist_albums(&mut self) {
        let Some(api) = self.api.clone() else { return };
        let Some(artist) = self.state.artist_details.artist() else {
            return;
        };
        let artist_id = artist.id.clone();
        let Some(request) = self
            .state
            .artist_details
            .begin_albums(&mut self.library_requests)
        else {
            return;
        };
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.artist_albums(&artist_id).await.map_err(error_message);
            let _ = tx.send(Msg::ArtistAlbums { request, result });
        });
    }

    /// Prefer stable artist IDs when the server advertises support; otherwise
    /// use the standard display-name request for compatibility.
    pub(in crate::app) fn load_artist_top_songs(&mut self) {
        let Some(api) = self.api.clone() else { return };
        let Some(artist) = self.state.artist_details.artist().cloned() else {
            return;
        };
        let artist_id_support = self
            .state
            .connection
            .server()
            .map(|server| server.capabilities.top_songs_by_artist_id)
            .unwrap_or_default();
        let Some(request) = self
            .state
            .artist_details
            .begin_top_songs(&mut self.library_requests)
        else {
            return;
        };
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api
                .artist_top_songs(&artist, 12, artist_id_support)
                .await
                .map_err(error_message);
            let _ = tx.send(Msg::ArtistTopSongs { request, result });
        });
    }

    pub(in crate::app) fn finish_artist_albums(
        &mut self,
        request: LibraryRequest,
        result: Result<Vec<Album>, String>,
    ) {
        let Some(result) = self.state.artist_details.complete_albums(request, result) else {
            return;
        };
        match result {
            Ok(albums) => {
                let covers = albums
                    .iter()
                    .filter_map(|album| album.cover_art.clone())
                    .collect();
                self.preload_covers(covers);
            }
            Err(error) => {
                let error = error.to_string();
                self.notify_error(error);
            }
        }
    }

    pub(in crate::app) fn finish_artist_top_songs(
        &mut self,
        request: LibraryRequest,
        result: Result<Vec<Song>, String>,
    ) {
        let Some(result) = self
            .state
            .artist_details
            .complete_top_songs(request, result)
        else {
            return;
        };
        let covers = result.ok().map(|songs| {
            songs
                .iter()
                .filter_map(|song| song.cover_art.clone())
                .take(12)
                .collect()
        });
        self.state
            .clear_song_selection_for_scope(SongScope::ArtistTop);
        if let Some(covers) = covers {
            self.preload_covers(covers);
        }
    }

    pub(in crate::app) fn start_artist_radio(&mut self) {
        if let (Some(artist), Some(initial_song)) = (
            self.state.artist_details.artist().cloned(),
            self.state.artist_details.top_songs().first().cloned(),
        ) {
            self.start_artist_radio_from_seed(artist, initial_song);
        }
    }
}

#[cfg(test)]
mod tests;
