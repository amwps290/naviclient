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
        let ui = &self.favorite_ui;
        let removing = matches!(ui.kind, Some(FavoriteDialogKind::Remove { .. }));
        let active_source = self.api.as_ref().map(|api| api.id()) == ui.source.as_ref();
        let disabled = ui.loading || ui.writing || ui.uncertain || !active_source;
        let mut content = v_flex()
            .w_full()
            .gap_3()
            .child(
                div()
                    .text_xs()
                    .text_color(palette(cx).text_secondary)
                    .child(ui.source_label.clone()),
            )
            .when(ui.loading, |this| {
                this.child(div().text_sm().child(t!("Loading favorite status…")))
            })
            .when(ui.writing, |this| {
                this.child(
                    div()
                        .text_sm()
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
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(snapshot.video.title.clone()),
            );
            match &ui.kind {
                Some(FavoriteDialogKind::Picker) => {
                    if snapshot.folders.is_empty() {
                        content = content.child(div().text_sm().child(t!(
                            "No owned favorite folders. Create one in Bilibili first."
                        )));
                    }
                    content = content.child(
                        v_flex()
                            .id("bilibili-favorite-folder-picker")
                            .max_h(px(280.0))
                            .overflow_y_scroll()
                            .gap_2()
                            .children(snapshot.folders.iter().map(|folder| {
                                let id = folder.id;
                                h_flex()
                                    .w_full()
                                    .gap_3()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_sm()
                                            .truncate()
                                            .child(folder.title.clone()),
                                    )
                                    .child(if folder.contains == Some(true) {
                                        div()
                                            .text_xs()
                                            .text_color(palette(cx).text_secondary)
                                            .child(t!("Already saved"))
                                            .into_any_element()
                                    } else if folder.contains.is_none() {
                                        div()
                                            .text_xs()
                                            .text_color(palette(cx).text_secondary)
                                            .child(t!("Favorite status unknown"))
                                            .into_any_element()
                                    } else {
                                        Button::new(SharedString::from(format!(
                                            "bilibili-save-to-folder-{id}"
                                        )))
                                        .label(t!("Save here"))
                                        .primary()
                                        .small()
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
                    content = content.child(div().text_sm().child(t!(
                        "Remove this video from “%{folder}”? All its parts will leave this folder; the video and current playback are not deleted.", folder = name)));
                    if snapshot.membership(*folder_id) == Some(false) {
                        content = content.child(
                            div()
                                .text_sm()
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
                    .text_sm()
                    .text_color(palette(cx).text_secondary)
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
                Button::new("check-bilibili-favorite-status")
                    .label(t!("Check favorite status"))
                    .ghost()
                    .disabled(ui.loading || ui.writing || !active_source)
                    .on_click(cx.listener(|this, _, _, cx| this.reload_bilibili_favorite(cx))),
            )
            .child(
                Button::new("close-bilibili-favorite")
                    .label(t!("Close"))
                    .outline()
                    .disabled(ui.writing)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.close_bilibili_favorite(window, cx)),
                    ),
            );
        if let Some(FavoriteDialogKind::Remove { folder_id, .. }) = &ui.kind {
            let id = *folder_id;
            footer = footer.child(
                Button::new("confirm-bilibili-favorite-removal")
                    .label(t!("Remove video"))
                    .danger()
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
