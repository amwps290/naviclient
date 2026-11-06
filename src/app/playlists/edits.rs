//! Playlist write orchestration and accepted result handling.

use super::edit_state::PlaylistEditCompletion;
use crate::app::prelude::*;
use crate::playlist::{PlaylistEdit, PlaylistEdited};

fn finish_playlist_workflow(
    completion: &PlaylistEditCompletion,
    playlist_ui: &mut PlaylistUi,
    import_ui: &mut ImportUi,
) -> bool {
    match completion.origin {
        PlaylistEditOrigin::Dialog => playlist_ui.complete_edit(&completion.result),
        PlaylistEditOrigin::Import => import_ui.complete_playlist_edit(&completion.result),
        PlaylistEditOrigin::Menu => false,
    }
}

impl NavidromeApp {
    pub(in crate::app) fn playlist_read_error(&mut self, error: String) {
        self.playlist_ui.set_error(error.clone());
        self.notify_error(error);
    }

    pub(in crate::app) fn start_playlist_edit(
        &mut self,
        edit: PlaylistEdit,
        origin: PlaylistEditOrigin,
        cx: &mut Context<Self>,
    ) {
        if self.state.playlists.editing() {
            return;
        }
        let Some(api) = self.api.clone() else { return };
        let Some(request) = self
            .state
            .playlists
            .begin_edit(&mut self.library_requests, origin)
        else {
            return;
        };
        match origin {
            PlaylistEditOrigin::Dialog => self.playlist_ui.error = None,
            PlaylistEditOrigin::Import => self.import_ui.flow.clear_error(),
            PlaylistEditOrigin::Menu => {}
        }
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.edit_playlist(&edit).await.map_err(error_message);
            let _ = tx.send(Msg::PlaylistEdited { request, result });
        });
        cx.notify();
    }

    pub(in crate::app) fn finish_playlist_edit(
        &mut self,
        request: LibraryRequest,
        result: Result<PlaylistEdited, String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(completion) = self.state.playlists.complete_edit(request, result) else {
            return;
        };
        // The request owns its workflow. A menu edit must not close or reset an
        // unrelated import, and an import failure keeps its review available.
        if finish_playlist_workflow(&completion, &mut self.playlist_ui, &mut self.import_ui) {
            window.close_dialog(cx);
        }
        match completion.result {
            Ok(edited) => {
                if edited.deleted {
                    if self.state.playlists.playlist().is_none()
                        && self.state.view == View::PlaylistDetail
                    {
                        self.state.view = View::Playlists;
                        self.state
                            .clear_song_selection_for_scope(SongScope::PlaylistDetail);
                    }
                    window.push_notification(Notification::success(t!("Playlist deleted")), cx);
                } else {
                    window.push_notification(Notification::success(t!("Playlist saved")), cx);
                }
            }
            Err(error) => {
                self.notify_error(error);
            }
        }
        self.load_playlists();
        self.load_playlist_detail();
        cx.notify();
    }
}

#[cfg(test)]
mod tests;
