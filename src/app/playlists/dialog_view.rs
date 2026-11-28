//! Rendering and actions for playlist naming, picking and confirmation.

use super::dialog::PlaylistDialog;
use crate::app::prelude::*;
use crate::playlist::{can_edit, PlaylistEdit};
use gpui_kit::component::dialog::Dialog;

impl NavidromeApp {
    pub(in crate::app) fn render_playlist_dialog(
        &self,
        dialog: Dialog,
        cx: &mut Context<Self>,
    ) -> Dialog {
        let p = palette(cx);
        let dark = cx.theme().is_dark();
        let panel_bg = if dark {
            hsla(0.0, 0.0, 1.0, 0.035)
        } else {
            p.sunken.opacity(0.65)
        };
        let panel_border = if dark {
            hsla(0.0, 0.0, 1.0, 0.08)
        } else {
            p.border.opacity(0.82)
        };
        let busy = self.state.playlists.editing();
        let mut content = v_flex().gap_3();
        let (title, action, destructive) = match &self.playlist_ui.dialog {
            Some(PlaylistDialog::Name {
                playlist,
                songs,
                input,
            }) => {
                let mut card = v_flex()
                    .w_full()
                    .p_3()
                    .gap_2p5()
                    .rounded(px(tokens::radius::COVER))
                    .bg(panel_bg)
                    .border_1()
                    .border_color(panel_border);
                if playlist.is_none() && !songs.is_empty() {
                    card = card.child(
                        div()
                            .text_xs()
                            .text_color(p.text_secondary)
                            .child(t!(
                                "Save %{count} tracks in their current order.",
                                count = songs.len()
                            )),
                    );
                }
                card = card.child(Input::new(input).disabled(busy).w_full());
                content = content.child(card);
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
                content = content.child(
                    h_flex()
                        .w_full()
                        .px_3()
                        .py_2p5()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .rounded(px(tokens::radius::COVER))
                        .bg(panel_bg)
                        .border_1()
                        .border_color(panel_border)
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(p.text_secondary)
                                .child(t!(
                                    "Add %{count} tracks. Existing copies will be kept.",
                                    count = songs.len()
                                )),
                        )
                        .child(
                            surface_button("new-playlist-from-songs")
                                .label(t!("New playlist…"))
                                .compact_action()
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.name_playlist(None, new_songs.clone(), window, cx)
                                })),
                        ),
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
                let mut picker = v_flex()
                    .id("playlist-picker")
                    .max_h(px(280.0))
                    .overflow_y_scroll()
                    .p_1p5()
                    .gap_1()
                    .rounded(px(tokens::radius::COVER))
                    .bg(panel_bg)
                    .border_1()
                    .border_color(panel_border);
                if self.state.playlists.list_loading() {
                    picker = picker.child(
                        div()
                            .py_6()
                            .text_center()
                            .text_sm()
                            .text_color(p.text_secondary)
                            .child(t!("Loading playlists…")),
                    );
                } else if playlists.is_empty() {
                    picker = picker.child(
                        div()
                            .py_6()
                            .px_4()
                            .text_center()
                            .text_sm()
                            .text_color(p.text_muted)
                            .child(t!("No editable playlists yet. Create one above.")),
                    );
                } else {
                    picker = picker.children(playlists.into_iter().map(|playlist| {
                        let songs = songs.clone();
                        let count = playlist.song_count.unwrap_or_default();
                        let cover = self.render_cover_styled(
                            playlist.cover_art.as_deref(),
                            34.0,
                            tokens::radius::CONTROL,
                            Vec::new(),
                        );
                        ghost_button(SharedString::from(format!(
                            "choose-playlist-{}",
                            playlist.id
                        )))
                        .w_full()
                        .h(px(48.0))
                        .px_2p5()
                        .rounded(px(tokens::radius::ROW))
                        .justify_start()
                        .disabled(busy || self.state.playlists.list_loading())
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .items_center()
                                .gap_2p5()
                                .child(cover)
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.text)
                                        .truncate()
                                        .child(playlist.name),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_xs()
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .text_color(p.text_muted)
                                        .child(t!("%{count} tracks", count = count)),
                                ),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.start_playlist_edit(
                                PlaylistEdit::Append {
                                    id: playlist.id.clone(),
                                    songs: songs.clone(),
                                },
                                PlaylistEditOrigin::Dialog,
                                cx,
                            );
                        }))
                    }));
                }
                content = content.child(picker);
                (t!("Add to playlist").to_string(), None, false)
            }
            Some(PlaylistDialog::Confirm {
                title,
                message,
                edit,
                ..
            }) => {
                content = content.child(
                    div()
                        .w_full()
                        .p_3()
                        .rounded(px(tokens::radius::ROW))
                        .bg(panel_bg)
                        .border_1()
                        .border_color(panel_border)
                        .text_sm()
                        .text_color(p.text_secondary)
                        .child(message.clone()),
                );
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
                    .px_3()
                    .py_2()
                    .rounded(px(tokens::radius::ROW))
                    .bg(cx.theme().danger.opacity(0.09))
                    .border_1()
                    .border_color(cx.theme().danger.opacity(0.22))
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            );
        }
        if busy {
            content = content.child(
                div()
                    .text_xs()
                    .text_color(p.text_secondary)
                    .child(t!("Saving…")),
            );
        }
        let footer = h_flex()
            .justify_end()
            .gap_2()
            .child(
                ghost_button("cancel-playlist-dialog")
                    .label(t!("Cancel"))
                    .dialog_action()
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.playlist_ui.close_dialog();
                        window.close_dialog(cx);
                        cx.notify();
                    })),
            )
            .when_some(action, |footer, action| {
                let button = if destructive {
                    danger_button("save-playlist-dialog")
                } else {
                    primary_button("save-playlist-dialog")
                };
                footer.child(
                    button
                        .label(action)
                        .dialog_action()
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
