//! Playlist import dialog rendering and review summaries.

use super::artwork_helpers::readable_accent;
use super::*;
use crate::import;
use gpui_kit::component::dialog::Dialog;

impl NavidromeApp {
    pub(super) fn render_import_dialog(&self, dialog: Dialog, cx: &mut Context<Self>) -> Dialog {
        let busy = self.import_busy();
        let counts = self
            .import_ui
            .flow
            .review()
            .map(|review| review.counts())
            .unwrap_or_default();
        let matched_count = counts.matched;
        let failure_count = counts.failed;
        let mut content = v_flex().gap_3();
        let action: Option<String> = match self.import_ui.flow.stage() {
            Some(ImportStage::Url) => {
                if let Some(input) = &self.import_ui.url_input {
                    content = content
                        .child(
                            div()
                                .text_sm()
                                .child(t!(
                                    "Paste a NetEase Cloud Music or QQ Music playlist link."
                                )),
                        )
                        .child(Input::new(input).w_full())
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!(
                                "Direct links, mobile share links, short links, and full share messages are supported."
                            )),
                        );
                }
                Some(t!("Fetch").to_string())
            }
            Some(ImportStage::Matching) => {
                let (done, total) = self.import_ui.flow.progress();
                let ratio = if total > 0 {
                    done as f64 / total as f64
                } else {
                    0.0
                };
                let label = if total > 0 {
                    t!(
                        "Matching tracks… %{done}/%{total}",
                        done = done,
                        total = total
                    )
                } else {
                    t!("Fetching playlist…")
                };
                let bar = readable_accent(cx.theme().accent, cx.theme().background);
                if let Some(playlist) = self.import_ui.flow.matching_playlist() {
                    content = content.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .child(playlist.name.clone()),
                    );
                }
                content = content.child(div().text_sm().child(label)).child(
                    div()
                        .h(px(6.0))
                        .w_full()
                        .rounded_full()
                        .bg(cx.theme().border)
                        .child(
                            div()
                                .h_full()
                                .rounded_full()
                                .w(relative(ratio as f32))
                                .bg(bar),
                        ),
                );
                None
            }
            Some(ImportStage::Review) => {
                let Some(review) = self.import_ui.flow.review() else {
                    return dialog;
                };
                let unmatched_count = counts.unmatched;
                let unavailable_count = review.unavailable;
                if let Some(input) = &self.import_ui.name_input {
                    content = content
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("Playlist name")),
                        )
                        .child(
                            Input::new(input)
                                .w_full()
                                .disabled(self.state.playlists.editing()),
                        );
                }
                content = content
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(cx.theme().success.opacity(0.12))
                                    .text_xs()
                                    .text_color(cx.theme().success)
                                    .child(t!("Matched %{count}", count = matched_count)),
                            )
                            .child(
                                div()
                                    .px_2()
                                    .py_1()
                                    .rounded_md()
                                    .bg(cx.theme().secondary)
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("No match %{count}", count = unmatched_count)),
                            )
                            .when(failure_count > 0, |summary| {
                                summary.child(
                                    div()
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .bg(cx.theme().danger.opacity(0.1))
                                        .text_xs()
                                        .text_color(cx.theme().danger)
                                        .child(t!("Check failed %{count}", count = failure_count)),
                                )
                            })
                            .when(unavailable_count > 0, |summary| {
                                summary.child(
                                    div()
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .bg(cx.theme().info.opacity(0.1))
                                        .text_xs()
                                        .text_color(cx.theme().foreground)
                                        .child(t!(
                                            "Unavailable %{count}",
                                            count = unavailable_count
                                        )),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("Only matched tracks will be imported.")),
                    );
                if failure_count > 0 {
                    content = content.child(
                        div()
                            .p_3()
                            .rounded_md()
                            .bg(cx.theme().danger.opacity(0.08))
                            .text_sm()
                            .text_color(cx.theme().danger)
                            .child(t!(
                                "Some tracks could not be checked because the server request failed. You can import the rest or cancel and try again."
                            )),
                    );
                }
                if unavailable_count > 0 {
                    content = content.child(
                        div()
                            .p_3()
                            .rounded_md()
                            .bg(cx.theme().info.opacity(0.1))
                            .text_sm()
                            .text_color(cx.theme().foreground)
                            .child(t!(
                                "%{count} tracks are unavailable from the source service and were skipped before matching.",
                                count = unavailable_count
                            )),
                    );
                }
                let rows: Vec<_> = review
                    .rows
                    .iter()
                    .map(|row| {
                        let track = &row.source;
                        let matched = row.outcome.song();
                        let (status, color, background) = match &row.outcome {
                            import::TrackMatch::Matched(_) => (
                                t!("Matched").to_string(),
                                cx.theme().success,
                                cx.theme().success.opacity(0.12),
                            ),
                            import::TrackMatch::Failed => (
                                t!("Check failed").to_string(),
                                cx.theme().danger,
                                cx.theme().danger.opacity(0.1),
                            ),
                            import::TrackMatch::Unmatched => (
                                t!("No match").to_string(),
                                cx.theme().muted_foreground,
                                cx.theme().secondary,
                            ),
                        };
                        let source = format!("{} · {}", track.title, track.artists.join(" / "));
                        v_flex()
                            .gap_1()
                            .p_3()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.7))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div().flex_1().min_w_0().text_sm().truncate().child(source),
                                    )
                                    .child(
                                        div()
                                            .px_2()
                                            .py_1()
                                            .rounded_md()
                                            .bg(background)
                                            .text_xs()
                                            .text_color(color)
                                            .child(status),
                                    ),
                            )
                            .when_some(matched, |row, song| {
                                row.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .truncate()
                                        .child(format!("→ {} · {}", song.title, song.artist)),
                                )
                            })
                    })
                    .collect();
                content = content.child(
                    v_flex()
                        .id("import-review-list")
                        .max_h(px(260.0))
                        .overflow_y_scroll()
                        .border_1()
                        .border_color(cx.theme().border)
                        .rounded_lg()
                        .children(rows),
                );
                Some(t!("Import %{count} tracks", count = matched_count).to_string())
            }
            None => return dialog,
        };
        if let Some(error) = self.import_ui.flow.error() {
            content = content.child(
                div()
                    .p_3()
                    .rounded_md()
                    .bg(cx.theme().danger.opacity(0.08))
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.to_string()),
            );
        }
        if self.state.playlists.editing() {
            content = content.child(
                div()
                    .p_3()
                    .rounded_md()
                    .bg(cx.theme().info.opacity(0.1))
                    .text_sm()
                    .text_color(cx.theme().foreground)
                    .child(t!("Saving…")),
            );
        }
        // Review 阶段没有任何匹配项时主按钮禁用（错误行会说明原因）。
        let review_empty =
            self.import_ui.flow.stage() == Some(ImportStage::Review) && matched_count == 0;
        let footer = h_flex()
            .justify_end()
            .gap_2()
            .child(
                Button::new("cancel-import-dialog")
                    .label(t!("Cancel"))
                    .outline()
                    .disabled(self.state.playlists.editing())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_import_dialog(window, cx);
                    })),
            )
            .when_some(action, |footer, action| {
                footer.child(
                    Button::new("submit-import-dialog")
                        .label(action)
                        .primary()
                        .disabled(busy || review_empty)
                        .on_click(cx.listener(|this, _, _, cx| this.submit_import_step(cx))),
                )
            });
        let submit = cx.entity().downgrade();
        let cancel = submit.clone();
        dialog
            .title(t!("Import playlist").to_string())
            .w(px(560.0))
            .overlay_closable(false)
            .close_button(!busy)
            .keyboard(!busy)
            .on_ok(move |_, _, cx| {
                let _ = submit.update(cx, |this, cx| this.submit_import_step(cx));
                false
            })
            .on_cancel(move |_, _, cx| {
                cancel
                    .update(cx, |this, _| !this.import_busy())
                    .unwrap_or(true)
            })
            .on_close(cx.listener(|this, _, _, cx| {
                this.import_ui.reset();
                cx.notify();
            }))
            .child(content)
            .footer(footer)
    }
}
