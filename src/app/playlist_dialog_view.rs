//! Rendering and actions for playlist naming, picking and confirmation.

use super::playlist_dialog::PlaylistDialog;
use super::*;
use crate::playlist::{can_edit, PlaylistEdit};
use gpui_kit::component::dialog::Dialog;

impl NavidromeApp {
    pub(super) fn render_playlist_dialog(&self, dialog: Dialog, cx: &mut Context<Self>) -> Dialog {
        let busy = self.state.playlists.editing();
        let mut content = v_flex().gap_3();
        let (title, action, destructive) = match &self.playlist_ui.dialog {
            Some(PlaylistDialog::Name {
                playlist,
                songs,
                input,
            }) => {
                if playlist.is_none() && !songs.is_empty() {
                    content = content.child(t!(
                        "Save %{count} tracks in their current order.",
                        count = songs.len()
                    ));
                }
                content = content.child(Input::new(input).disabled(busy).w_full());
                (
                    if playlist.is_some() {
                        t!("Rename playlist")
                    } else {
                        t!("New playlist")
                    }
                    .to_string(),
                    Some(t!("Save").to_string()),
                    false,
                )
            }
            Some(PlaylistDialog::Choose { songs }) => {
                let new_songs = songs.clone();
                content = content
                    .child(t!(
                        "Add %{count} tracks. Existing copies will be kept.",
                        count = songs.len()
                    ))
                    .child(
                        Button::new("new-playlist-from-songs")
                            .label(t!("New playlist…"))
                            .outline()
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.name_playlist(None, new_songs.clone(), window, cx)
                            })),
                    );
                let mut playlists: Vec<_> = self
                    .state
                    .playlists
                    .items()
                    .iter()
                    .filter(|p| can_edit(p, &self.config.username))
                    .cloned()
                    .collect();
                playlists.sort_by_key(|p| p.name.to_lowercase());
                if self.state.playlists.list_loading() {
                    content = content.child(div().text_sm().child(t!("Loading playlists…")));
                } else if playlists.is_empty() {
                    content = content.child(
                        div()
                            .text_sm()
                            .child(t!("No editable playlists yet. Create one above.")),
                    );
                }
                content = content.child(
                    v_flex()
                        .id("playlist-picker")
                        .max_h(px(260.0))
                        .overflow_y_scroll()
                        .gap_1()
                        .children(playlists.into_iter().map(|playlist| {
                            let songs = songs.clone();
                            Button::new(SharedString::from(format!(
                                "choose-playlist-{}",
                                playlist.id
                            )))
                            .label(playlist.name)
                            .ghost()
                            .w_full()
                            .justify_start()
                            .disabled(busy || self.state.playlists.list_loading())
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.start_playlist_edit(
                                        PlaylistEdit::Append {
                                            id: playlist.id.clone(),
                                            songs: songs.clone(),
                                        },
                                        PlaylistEditOrigin::Dialog,
                                        cx,
                                    );
                                },
                            ))
                        })),
                );
                (t!("Add to playlist").to_string(), None, false)
            }
            Some(PlaylistDialog::Confirm {
                title,
                message,
                edit,
            }) => {
                content = content.child(div().text_sm().child(message.clone()));
                (
                    title.clone(),
                    Some(if matches!(edit, PlaylistEdit::Delete { .. }) {
                        t!("Delete").to_string()
                    } else {
                        t!("Remove").to_string()
                    }),
                    true,
                )
            }
            None => return dialog,
        };
        if let Some(error) = &self.playlist_ui.error {
            content = content.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            );
        }
        if busy {
            content = content.child(div().text_sm().child(t!("Saving…")));
        }
        let footer = h_flex()
            .justify_end()
            .gap_2()
            .child(
                Button::new("cancel-playlist-dialog")
                    .label(t!("Cancel"))
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.playlist_ui.close_dialog();
                        window.close_dialog(cx);
                        cx.notify();
                    })),
            )
            .when_some(action, |footer, action| {
                footer.child(
                    Button::new("save-playlist-dialog")
                        .label(action)
                        .primary()
                        .when(destructive, |button| button.danger())
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| this.submit_playlist_dialog(cx))),
                )
            });
        let submit = cx.entity().downgrade();
        let cancel = submit.clone();
        dialog
            .title(title)
            .w(px(440.0))
            .overlay_closable(false)
            .close_button(!busy)
            .keyboard(!busy)
            .on_ok(move |_, _, cx| {
                let _ = submit.update(cx, |this, cx| this.submit_playlist_dialog(cx));
                false
            })
            .on_cancel(move |_, _, cx| {
                cancel
                    .update(cx, |this, _| !this.state.playlists.editing())
                    .unwrap_or(true)
            })
            .on_close(cx.listener(|this, _, _, cx| {
                this.playlist_ui.close_dialog();
                cx.notify();
            }))
            .child(content)
            .footer(footer)
    }
}
