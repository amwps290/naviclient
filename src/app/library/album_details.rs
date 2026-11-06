//! Selected album data and requests, independent of deferred playback.

use super::helpers::sort_album_songs;
use super::requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use crate::app::prelude::*;

#[derive(Default)]
pub(in crate::app) struct AlbumDetails {
    album: Option<Album>,
    songs: Vec<Song>,
    load: LibraryLoad,
}

impl AlbumDetails {
    pub(in crate::app) fn album(&self) -> Option<&Album> {
        self.album.as_ref()
    }

    pub(in crate::app) fn songs(&self) -> &[Song] {
        &self.songs
    }

    pub(in crate::app) fn loading(&self) -> bool {
        self.load.loading()
    }

    pub(in crate::app) fn failed(&self) -> bool {
        self.load.error().is_some()
    }

    pub(in crate::app) fn select(&mut self, album: Album) {
        self.album = Some(album);
        self.songs.clear();
        self.load = LibraryLoad::default();
    }

    pub(in crate::app) fn begin(
        &mut self,
        requests: &mut LibraryRequests,
    ) -> Option<LibraryRequest> {
        self.album.as_ref()?;
        Some(self.load.begin(requests))
    }

    pub(in crate::app) fn complete(
        &mut self,
        request: LibraryRequest,
        result: Result<Vec<Song>, String>,
    ) -> Option<Result<&[Song], &str>> {
        Some(match self.load.complete(request, result)? {
            Ok(mut songs) => {
                sort_album_songs(&mut songs);
                self.songs = songs;
                Ok(&self.songs)
            }
            Err(error) => Err(error),
        })
    }
}

impl NavidromeApp {
    pub(in crate::app) fn open_album(&mut self, album: Album) {
        self.clear_song_multi();
        self.ensure_cover(album.cover_art.as_deref(), true);
        self.state.album_details.select(album);
        self.state.view = View::AlbumDetail;
        self.load_album_details();
    }

    pub(in crate::app) fn load_album_details(&mut self) {
        let Some(api) = self.api.clone() else { return };
        let Some(album) = self.state.album_details.album() else {
            return;
        };
        let album_id = album.id.clone();
        let Some(request) = self.state.album_details.begin(&mut self.library_requests) else {
            return;
        };
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.album_songs(&album_id).await.map_err(error_message);
            let _ = tx.send(Msg::AlbumSongs { request, result });
        });
    }

    pub(in crate::app) fn finish_album_details(
        &mut self,
        request: LibraryRequest,
        result: Result<Vec<Song>, String>,
    ) {
        let Some(result) = self.state.album_details.complete(request, result) else {
            return;
        };
        match result {
            Ok(songs) => {
                let covers = songs
                    .iter()
                    .filter_map(|song| song.cover_art.clone())
                    .take(40)
                    .collect();
                self.state
                    .clear_song_selection_for_scope(SongScope::AlbumDetail);
                self.preload_covers(covers);
            }
            Err(error) => {
                let error = error.to_string();
                self.notify_error(error);
            }
        }
    }
}

#[cfg(test)]
mod tests;
