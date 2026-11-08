//! Playlist read orchestration and applying accepted results to the UI.

use crate::app::prelude::*;
use crate::playlist::PlaylistDetails;

impl NavidromeApp {
    pub(in crate::app) fn load_playlists(&mut self) {
        let Some(api) = self.api.clone() else { return };
        let request = self.state.playlists.begin_list(&mut self.library_requests);
        let tx = self.tx.clone();
        let task = self.runtime.spawn(async move {
            let result = api.playlists().await.map_err(error_message);
            let _ = tx.send(Msg::Playlists { request, result });
        });
        self.state.playlists.attach_list(task.abort_handle());
    }

    pub(in crate::app) fn finish_playlists(
        &mut self,
        request: LibraryRequest,
        result: Result<Vec<Playlist>, String>,
    ) {
        let Some(result) = self.state.playlists.complete_list(request, result) else {
            return;
        };
        match result {
            Ok(playlists) => {
                let covers = playlists
                    .iter()
                    .filter_map(|playlist| playlist.cover_art.clone())
                    .collect();
                self.preload_covers(covers);
            }
            Err(error) => {
                let error = error.to_string();
                self.playlist_read_error(error);
            }
        }
    }

    pub(in crate::app) fn open_playlist(&mut self, playlist: Playlist) {
        self.clear_song_multi();
        self.ensure_cover(playlist.cover_art.as_deref(), true);
        self.state.playlists.select(playlist);
        self.state.view = View::PlaylistDetail;
        self.load_playlist_detail();
    }

    pub(in crate::app) fn load_playlist_detail(&mut self) {
        let Some(api) = self.api.clone() else { return };
        let Some(playlist) = self.state.playlists.playlist() else {
            return;
        };
        let playlist_id = playlist.id.clone();
        let Some(request) = self
            .state
            .playlists
            .begin_detail(&mut self.library_requests)
        else {
            return;
        };
        let tx = self.tx.clone();
        let task = self.runtime.spawn(async move {
            let result = api.playlist(&playlist_id).await.map_err(error_message);
            let _ = tx.send(Msg::PlaylistSongs { request, result });
        });
        self.state.playlists.attach_detail(task.abort_handle());
    }

    pub(in crate::app) fn finish_playlist_detail(
        &mut self,
        request: LibraryRequest,
        result: Result<PlaylistDetails, String>,
    ) {
        let Some(result) = self.state.playlists.complete_detail(request, result) else {
            return;
        };
        match result {
            Ok(details) => {
                let unavailable = details.unavailable_count;
                let cover = details.playlist.cover_art.clone();
                let covers = details
                    .songs
                    .iter()
                    .filter_map(|song| song.cover_art.clone())
                    .take(40)
                    .collect();
                self.state
                    .clear_song_selection_for_scope(SongScope::PlaylistDetail);
                self.ensure_cover(cover.as_deref(), true);
                self.preload_covers(covers);
                if unavailable > 0 {
                    self.notify_warning(t!(
                        "Skipped %{count} unavailable favorites",
                        count = unavailable
                    ));
                }
            }
            Err(error) => {
                let error = error.to_string();
                self.notify_error(error);
            }
        }
    }
}
