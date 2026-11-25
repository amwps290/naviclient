//! Playlist read orchestration and applying accepted results to the UI.

use super::library::PlaylistDetailChange;
use crate::app::prelude::*;
use crate::playlist::loading::playlist_update_channel;

const DETAIL_UPDATES_PER_POLL: usize = 16;
const DETAIL_UPDATE_BUDGET: Duration = Duration::from_millis(4);

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
        mut result: Result<Vec<Playlist>, String>,
    ) {
        if self.browsing_bilibili() {
            if let Ok(folders) = &mut result {
                for folder in folders {
                    if folder.cover_art.is_none()
                        && folder.song_count.is_some_and(|count| count > 0)
                    {
                        folder.cover_art = self
                            .state
                            .playlists
                            .items()
                            .iter()
                            .find(|previous| previous.id == folder.id)
                            .and_then(|previous| previous.cover_art.clone());
                    }
                }
            }
        }
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
        if self.browsing_bilibili()
            && self
                .state
                .playlists
                .items()
                .iter()
                .any(|folder| folder.id == playlist.id)
        {
            self.state.bilibili.visit(&playlist.id);
        }
        self.clear_song_multi();
        self.song_list_scrolls
            .borrow_mut()
            .insert(SongScope::PlaylistDetail, UniformListScrollHandle::new());
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
        let source = api.id().clone();
        let (updates, receiver) = playlist_update_channel();
        let tx = self.tx.clone();
        let task = self.runtime.spawn(super::search::load_indexed_playlist(
            api,
            playlist_id,
            request,
            updates,
            tx,
        ));
        self.state
            .playlists
            .attach_detail_stream(request, source, receiver, task.abort_handle());
    }

    pub(in crate::app) fn poll_playlist_detail(&mut self) -> bool {
        let deadline = std::time::Instant::now() + DETAIL_UPDATE_BUDGET;
        let mut changed = false;
        for _ in 0..DETAIL_UPDATES_PER_POLL {
            let Some((request, update)) = self.state.playlists.next_detail_update() else {
                break;
            };
            let Some(change) = self.state.playlists.apply_detail_update(request, update) else {
                continue;
            };
            changed = true;
            let reset = match change {
                PlaylistDetailChange::Updated { reset } => reset,
                PlaylistDetailChange::Complete { reset, unavailable } => {
                    if unavailable > 0 {
                        self.notify_warning(t!(
                            "Skipped %{count} unavailable favorites",
                            count = unavailable
                        ));
                    }
                    reset
                }
                PlaylistDetailChange::Failed { reset, message } => {
                    self.notify_error(message);
                    reset
                }
            };
            if reset {
                self.state
                    .clear_song_selection_for_scope(SongScope::PlaylistDetail);
            }
            if std::time::Instant::now() >= deadline {
                break;
            }
        }
        if changed && self.state.view == View::PlaylistDetail {
            let cover = self
                .state
                .playlists
                .playlist()
                .and_then(|playlist| playlist.cover_art.clone());
            self.ensure_cover(cover.as_deref(), true);
            self.load_visible_song_covers(SongScope::PlaylistDetail);
        }
        changed
    }
}
