//! Owned-folder picker and whole-video removal confirmation.

use super::favorite_state::{FavoriteDialogKind, FavoriteFeedback};
use crate::app::prelude::*;
use crate::source::bilibili::favorites::FavoriteOperation;
use gpui_kit::component::dialog::Dialog;

impl NavidromeApp {
    pub(in crate::app) fn render_bilibili_favorite_dialog(
        &self,
        dialog: Dialog,
        cx: &Context<Self>,
    ) -> Dialog {
        let p = palette(cx);
        let ui = &self.favorite_ui;
        let removing = matches!(ui.kind, Some(FavoriteDialogKind::Remove { .. }));
        let active_source = self.api.as_ref().map(|api| api.id()) == ui.source.as_ref();
        let disabled = ui.loading || ui.writing || ui.uncertain || !active_source;
        let mut content = v_flex()
            .w_full()
            .gap_3()
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(div().size(px(6.0)).rounded_full().bg(if active_source {
                        p.online
                    } else {
                        p.text_muted
                    }))
                    .child(
                        div()
                            .text_size(px(tokens::CAPTION))
                            .text_color(p.text_secondary)
                            .child(ui.source_label.clone()),
                    ),
            )
            .when(ui.loading, |this| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(p.text_secondary)
                        .child(t!("Loading favorite status…")),
                )
            })
            .when(ui.writing, |this| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(p.text_secondary)
                        .child(t!("Saving favorite… Do not close the app.")),
                )
            })
            .when(!active_source, |this| {
                this.child(div().text_sm().text_color(cx.theme().danger).child(t!(
                    "The music source changed. Close this dialog and open it again."
                )))
            });
        if let Some(snapshot) = &ui.snapshot {
            content = content.child(
                div()
                    .w_full()
                    .px_3()
                    .py_2p5()
                    .rounded(px(tokens::radius::ROW))
                    .bg(p.sunken)
                    .text_size(px(tokens::BODY))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(snapshot.video.title.clone()),
            );
            match &ui.kind {
                Some(FavoriteDialogKind::Picker) => {
                    if snapshot.folders.is_empty() {
                        content =
                            content.child(div().text_sm().text_color(p.text_secondary).child(t!(
                                "No owned favorite folders. Create one in Bilibili first."
                            )));
                    }
                    content = content.child(
                        v_flex()
                            .id("bilibili-favorite-folder-picker")
                            .max_h(px(280.0))
                            .overflow_y_scroll()
                            .gap_1()
                            .children(snapshot.folders.iter().map(|folder| {
                                let id = folder.id;
                                h_flex()
                                    .w_full()
                                    .min_h(px(40.0))
                                    .px_3()
                                    .py_1p5()
                                    .gap_3()
                                    .items_center()
                                    .rounded(px(tokens::radius::ROW))
                                    .hover(|style| style.bg(p.sunken))
                                    .child(
                                        Icon::new(AppIcon::Playlists)
                                            .with_size(px(15.0))
                                            .text_color(p.text_muted),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(px(tokens::BODY))
                                            .truncate()
                                            .child(folder.title.clone()),
                                    )
                                    .child(if folder.contains == Some(true) {
                                        div()
                                            .flex_none()
                                            .px_2p5()
                                            .py_0p5()
                                            .rounded_full()
                                            .bg(p.brand_soft)
                                            .text_size(px(tokens::CAPTION))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(p.brand)
                                            .child(t!("Already saved"))
                                            .into_any_element()
                                    } else if folder.contains.is_none() {
                                        div()
                                            .flex_none()
                                            .px_2p5()
                                            .py_0p5()
                                            .rounded_full()
                                            .bg(p.sunken)
                                            .text_size(px(tokens::CAPTION))
                                            .text_color(p.text_muted)
                                            .child(t!("Favorite status unknown"))
                                            .into_any_element()
                                    } else {
                                        primary_button(SharedString::from(format!(
                                            "bilibili-save-to-folder-{id}"
                                        )))
                                        .label(t!("Save here"))
                                        .rounded(px(tokens::radius::FULL))
                                        .compact_action()
                                        .disabled(disabled)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.submit_bilibili_favorite(
                                                FavoriteOperation {
                                                    folder_id: id,
                                                    present: true,
                                                },
                                                cx,
                                            );
                                        }))
                                        .into_any_element()
                                    })
                                    .into_any_element()
                            })),
                    );
                }
                Some(FavoriteDialogKind::Remove {
                    folder_id,
                    folder_name,
                }) => {
                    let name = snapshot
                        .folders
                        .iter()
                        .find(|folder| folder.id == *folder_id)
                        .map(|folder| folder.title.as_str())
                        .unwrap_or(folder_name);
                    content = content.child(
                        div()
                            .text_sm()
                            .text_color(p.text_secondary)
                            .child(t!(
                                "Remove this video from “%{folder}”? All its parts will leave this folder; the video and current playback are not deleted.",
                                folder = name
                            )),
                    );
                    if snapshot.membership(*folder_id) == Some(false) {
                        content = content.child(
                            div()
                                .text_sm()
                                .text_color(p.text_secondary)
                                .child(t!("This video is no longer in this folder.")),
                        );
                    }
                }
                None => {}
            }
        }
        if let Some(feedback) = ui.feedback {
            content = content.child(
                div()
                    .px_3()
                    .py_2()
                    .rounded(px(tokens::radius::CONTROL))
                    .bg(p.sunken)
                    .text_sm()
                    .text_color(p.text_secondary)
                    .child(match feedback {
                        FavoriteFeedback::Saved(true) => t!("Favorite saved and verified."),
                        FavoriteFeedback::Saved(false) => {
                            t!("Video removed from this folder and verified.")
                        }
                        FavoriteFeedback::Checked => {
                            t!("Current favorite status checked. You can choose an action again.")
                        }
                        FavoriteFeedback::Uncertain => {
                            t!("The result is not confirmed. Check status before trying again.")
                        }
                    }),
            );
        }
        if let Some(error) = &ui.error {
            content = content.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.clone()),
            );
        }
        let mut footer = h_flex()
            .justify_end()
            .items_center()
            .gap_2()
            .child(
                ghost_button("check-bilibili-favorite-status")
                    .label(t!("Check favorite status"))
                    .dialog_action()
                    .disabled(ui.loading || ui.writing || !active_source)
                    .on_click(cx.listener(|this, _, _, cx| this.reload_bilibili_favorite(cx))),
            )
            .child(
                ghost_button("close-bilibili-favorite")
                    .label(t!("Close"))
                    .dialog_action()
                    .disabled(ui.writing)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.close_bilibili_favorite(window, cx)),
                    ),
            );
        if let Some(FavoriteDialogKind::Remove { folder_id, .. }) = &ui.kind {
            let id = *folder_id;
            footer = footer.child(
                danger_button("confirm-bilibili-favorite-removal")
                    .label(t!("Remove video"))
                    .dialog_action()
                    .disabled(
                        disabled
                            || ui
                                .snapshot
                                .as_ref()
                                .and_then(|snapshot| snapshot.membership(id))
                                != Some(true),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.submit_bilibili_favorite(
                            FavoriteOperation {
                                folder_id: id,
                                present: false,
                            },
                            cx,
                        )
                    })),
            );
        }
        dialog
            .title(if removing {
                t!("Remove from this favorite folder")
            } else {
                t!("Save video to…")
            })
            .w(px(520.0))
            .overlay_closable(false)
            .keyboard(!ui.writing)
            .close_button(!ui.writing)
            .child(content)
            .footer(footer)
    }
}
