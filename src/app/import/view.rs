//! Playlist import dialog rendering and review summaries.

use crate::app::prelude::*;
use crate::import;
use gpui_kit::component::dialog::Dialog;

impl NavidromeApp {
    pub(in crate::app) fn render_import_dialog(
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
        let hairline = if dark {
            hsla(0.0, 0.0, 1.0, 0.06)
        } else {
            p.border.opacity(0.72)
        };
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
                    content = content.child(
                        v_flex()
                            .w_full()
                            .p_3p5()
                            .gap_2p5()
                            .rounded(px(tokens::radius::COVER))
                            .bg(panel_bg)
                            .border_1()
                            .border_color(panel_border)
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.text)
                                    .child(t!(
                                        "Paste a NetEase Cloud Music or QQ Music playlist link."
                                    )),
                            )
                            .child(Input::new(input).w_full())
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.text_muted)
                                    .child(t!(
                                    "Direct links, mobile share links, short links, and full share messages are supported."
                                )),
                            ),
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
                let pct = format!("{}%", (ratio * 100.0).round() as u32);
                let mut progress_card = v_flex()
                    .w_full()
                    .p_4()
                    .gap_2p5()
                    .rounded(px(tokens::radius::COVER))
                    .bg(panel_bg)
                    .border_1()
                    .border_color(panel_border);
                if let Some(playlist) = self.import_ui.flow.matching_playlist() {
                    progress_card = progress_card.child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .truncate()
                            .child(playlist.name.clone()),
                    );
                }
                progress_card = progress_card
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_2()
                            .child(div().text_xs().text_color(p.text_secondary).child(label))
                            .when(total > 0, |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.brand)
                                        .child(pct),
                                )
                            }),
                    )
                    .child(
                        div()
                            .h(px(6.0))
                            .w_full()
                            .rounded_full()
                            .bg(hairline)
                            .overflow_hidden()
                            .child(
                                div()
                                    .h_full()
                                    .rounded_full()
                                    .w(relative(ratio as f32))
                                    .bg(p.brand),
                            ),
                    );
                content = content.child(progress_card);
                None
            }
            Some(ImportStage::Review) => {
                let Some(review) = self.import_ui.flow.review() else {
                    return dialog;
                };
                let unmatched_count = counts.unmatched;
                let unavailable_count = review.unavailable;
                let pill = |text: SharedString, fg: Hsla, bg: Hsla| {
                    div()
                        .px_2p5()
                        .py_0p5()
                        .rounded_full()
                        .bg(bg)
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(fg)
                        .child(text)
                };
                let mut review_header = v_flex()
                    .w_full()
                    .p_3()
                    .gap_2p5()
                    .rounded(px(tokens::radius::COVER))
                    .bg(panel_bg)
                    .border_1()
                    .border_color(panel_border);
                if let Some(input) = &self.import_ui.name_input {
                    review_header = review_header
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.text_muted)
                                .child(t!("Playlist name")),
                        )
                        .child(
                            Input::new(input)
                                .w_full()
                                .disabled(self.state.playlists.editing()),
                        );
                }
                review_header = review_header
                    .child(
                        h_flex()
                            .flex_wrap()
                            .items_center()
                            .gap_1p5()
                            .child(pill(
                                t!("Matched %{count}", count = matched_count).into(),
                                p.online,
                                p.online.opacity(if dark { 0.16 } else { 0.12 }),
                            ))
                            .child(pill(
                                t!("No match %{count}", count = unmatched_count).into(),
                                p.text_secondary,
                                if dark {
                                    hsla(0.0, 0.0, 1.0, 0.06)
                                } else {
                                    p.raised.opacity(0.85)
                                },
                            ))
                            .when(failure_count > 0, |summary| {
                                summary.child(pill(
                                    t!("Check failed %{count}", count = failure_count).into(),
                                    cx.theme().danger,
                                    cx.theme().danger.opacity(0.12),
                                ))
                            })
                            .when(unavailable_count > 0, |summary| {
                                summary.child(pill(
                                    t!("Unavailable %{count}", count = unavailable_count).into(),
                                    p.text_secondary,
                                    p.brand_soft,
                                ))
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(p.text_muted)
                            .child(t!("Only matched tracks will be imported.")),
                    );
                content = content.child(review_header);
                if failure_count > 0 {
                    content = content.child(
                        div()
                            .px_3()
                            .py_2p5()
                            .rounded(px(tokens::radius::ROW))
                            .bg(cx.theme().danger.opacity(0.09))
                            .border_1()
                            .border_color(cx.theme().danger.opacity(0.22))
                            .text_xs()
                            .text_color(cx.theme().danger)
                            .child(t!(
                                "Some tracks could not be checked because the server request failed. You can import the rest or cancel and try again."
                            )),
                    );
                }
                if unavailable_count > 0 {
                    content = content.child(
                        div()
                            .px_3()
                            .py_2p5()
                            .rounded(px(tokens::radius::ROW))
                            .bg(panel_bg)
                            .border_1()
                            .border_color(panel_border)
                            .text_xs()
                            .text_color(p.text_secondary)
                            .child(t!(
                                "%{count} tracks are unavailable from the source service and were skipped before matching.",
                                count = unavailable_count
                            )),
                    );
                }
                let mut review_list = v_flex()
                    .id("import-review-list")
                    .max_h(px(280.0))
                    .overflow_y_scroll()
                    .rounded(px(tokens::radius::COVER))
                    .bg(panel_bg)
                    .border_1()
                    .border_color(panel_border);
                for (index, row) in review.rows.iter().enumerate() {
                    if index > 0 {
                        review_list = review_list.child(div().w_full().h(px(1.0)).bg(hairline));
                    }
                    let track = &row.source;
                    let matched = row.outcome.song();
                    let (status, color, background) = match &row.outcome {
                        import::TrackMatch::Matched(_) => (
                            t!("Matched").to_string(),
                            p.online,
                            p.online.opacity(if dark { 0.16 } else { 0.12 }),
                        ),
                        import::TrackMatch::Failed => (
                            t!("Check failed").to_string(),
                            cx.theme().danger,
                            cx.theme().danger.opacity(0.12),
                        ),
                        import::TrackMatch::Unmatched => (
                            t!("No match").to_string(),
                            p.text_muted,
                            if dark {
                                hsla(0.0, 0.0, 1.0, 0.06)
                            } else {
                                p.raised.opacity(0.85)
                            },
                        ),
                    };
                    let source = format!("{} · {}", track.title, track.artists.join(" / "));
                    review_list = review_list.child(
                        v_flex()
                            .gap_0p5()
                            .px_3()
                            .py_2p5()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .w_full()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_sm()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(p.text)
                                            .truncate()
                                            .child(source),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_full()
                                            .bg(background)
                                            .text_xs()
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(color)
                                            .child(status),
                                    ),
                            )
                            .when_some(matched, |row, song| {
                                row.child(
                                    div()
                                        .text_xs()
                                        .text_color(p.text_secondary)
                                        .truncate()
                                        .child(format!("→ {} · {}", song.title, song.artist)),
                                )
                            }),
                    );
                }
                content = content.child(review_list);
                Some(t!("Import %{count} tracks", count = matched_count).to_string())
            }
            None => return dialog,
        };
        if let Some(error) = self.import_ui.flow.error() {
            content = content.child(
                div()
                    .px_3()
                    .py_2p5()
                    .rounded(px(tokens::radius::ROW))
                    .bg(cx.theme().danger.opacity(0.09))
                    .border_1()
                    .border_color(cx.theme().danger.opacity(0.22))
                    .text_sm()
                    .text_color(cx.theme().danger)
                    .child(error.to_string()),
            );
        }
        if self.state.playlists.editing() {
            content = content.child(
                div()
                    .px_3()
                    .py_2()
                    .rounded(px(tokens::radius::ROW))
                    .bg(panel_bg)
                    .border_1()
                    .border_color(panel_border)
                    .text_sm()
                    .text_color(p.text_secondary)
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
                ghost_button("cancel-import-dialog")
                    .label(t!("Cancel"))
                    .dialog_action()
                    .disabled(self.state.playlists.editing())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.close_import_dialog(window, cx);
                    })),
            )
            .when_some(action, |footer, action| {
                footer.child(
                    primary_button("submit-import-dialog")
                        .label(action)
                        .dialog_action()
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
