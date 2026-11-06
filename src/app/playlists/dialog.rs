//! Playlist dialog data, input subscriptions and focus orchestration.

use crate::app::prelude::*;
use crate::playlist::{PlaylistEdit, PlaylistEdited};
use gpui_kit::FocusHandle;

#[derive(Default)]
pub(in crate::app) struct PlaylistUi {
    pub menu_focus: Option<FocusHandle>,
    pub(in crate::app) dialog: Option<PlaylistDialog>,
    pub(in crate::app) error: Option<String>,
    pub(in crate::app) input_subscription: Option<Subscription>,
}

impl PlaylistUi {
    /// 对话框是否打开（导入向导等兄弟模块只关心这个布尔）。
    pub fn dialog_open(&self) -> bool {
        self.dialog.is_some()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub(in crate::app) fn close_dialog(&mut self) -> bool {
        let open = self.dialog.take().is_some();
        self.input_subscription = None;
        self.error = None;
        open
    }

    pub(in crate::app) fn set_error(&mut self, error: String) {
        if self.dialog_open() {
            self.error = Some(error);
        }
    }

    /// Returns whether the owning window dialog should be closed.
    pub(in crate::app) fn complete_edit(
        &mut self,
        result: &Result<PlaylistEdited, String>,
    ) -> bool {
        match result {
            Ok(_) => self.close_dialog(),
            Err(error) => {
                self.set_error(error.clone());
                false
            }
        }
    }
}

pub(in crate::app) enum PlaylistDialog {
    Name {
        playlist: Option<Playlist>,
        songs: Vec<String>,
        input: Entity<InputState>,
    },
    Choose {
        songs: Vec<String>,
    },
    Confirm {
        edit: PlaylistEdit,
        title: String,
        message: String,
    },
}

impl NavidromeApp {
    pub(in crate::app) fn name_playlist(
        &mut self,
        playlist: Option<Playlist>,
        songs: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.playlists.editing()
            || self.api.is_none()
            || self.import_ui.flow.stage().is_some()
        {
            return;
        }
        let name = playlist
            .as_ref()
            .map_or("", |p| p.name.as_str())
            .to_string();
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("Playlist name"))
                .default_value(name)
        });
        self.playlist_ui.input_subscription =
            Some(cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit_playlist_dialog(cx);
                }
            }));
        self.playlist_ui.dialog = Some(PlaylistDialog::Name {
            playlist,
            songs,
            input: input.clone(),
        });
        self.playlist_ui.error = None;
        self.show_playlist_dialog(window, cx);
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    pub(in crate::app) fn choose_playlist(
        &mut self,
        songs: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.playlists.editing()
            || self.api.is_none()
            || songs.is_empty()
            || self.import_ui.flow.stage().is_some()
        {
            return;
        }
        self.playlist_ui.dialog = Some(PlaylistDialog::Choose { songs });
        self.playlist_ui.error = None;
        self.load_playlists();
        self.show_playlist_dialog(window, cx);
    }

    pub(in crate::app) fn confirm_playlist_edit(
        &mut self,
        edit: PlaylistEdit,
        title: String,
        message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.state.playlists.editing() || self.import_ui.flow.stage().is_some() {
            return;
        }
        self.playlist_ui.dialog = Some(PlaylistDialog::Confirm {
            edit,
            title,
            message,
        });
        self.playlist_ui.error = None;
        self.show_playlist_dialog(window, cx);
    }

    pub(in crate::app) fn delete_playlist(
        &mut self,
        playlist: Playlist,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.confirm_playlist_edit(
            PlaylistEdit::Delete { id: playlist.id },
            t!("Delete playlist?").to_string(),
            t!(
                "Delete “%{name}” from Navidrome? The music files will be kept.",
                name = playlist.name
            )
            .to_string(),
            window,
            cx,
        );
    }

    fn show_playlist_dialog(&self, window: &mut Window, cx: &mut Context<Self>) {
        if !window.has_active_dialog(cx) {
            let app = cx.entity().downgrade();
            // Let the menu finish dismissing before installing the dialog's
            // focus scope. Otherwise its focus restoration can swallow Enter/Escape.
            window.defer(cx, move |window, cx| {
                let render_app = app.clone();
                window.open_dialog(cx, move |dialog, _, cx| {
                    render_app
                        .update(cx, |this, cx| this.render_playlist_dialog(dialog, cx))
                        .expect("app owns dialog")
                });
                let _ = app.update(cx, |this, cx| {
                    if let Some(PlaylistDialog::Name { input, .. }) = &this.playlist_ui.dialog {
                        input.update(cx, |input, cx| input.focus(window, cx));
                    }
                });
            });
        }
        cx.notify();
    }

    pub(in crate::app) fn submit_playlist_dialog(&mut self, cx: &mut Context<Self>) {
        let edit = match &self.playlist_ui.dialog {
            Some(PlaylistDialog::Name {
                playlist,
                songs,
                input,
            }) => {
                let name = input.read(cx).value().trim().to_string();
                if name.is_empty() {
                    self.playlist_ui.error = Some(t!("Enter a playlist name.").to_string());
                    cx.notify();
                    return;
                }
                if let Some(playlist) = playlist {
                    PlaylistEdit::Rename {
                        id: playlist.id.clone(),
                        name,
                    }
                } else {
                    PlaylistEdit::Create {
                        name,
                        songs: songs.clone(),
                    }
                }
            }
            Some(PlaylistDialog::Confirm { edit, .. }) => edit.clone(),
            _ => return,
        };
        self.start_playlist_edit(edit, PlaylistEditOrigin::Dialog, cx);
    }
}
