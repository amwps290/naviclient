//! Home Daily Mix card and its playback, refresh and append controls.

use crate::app::prelude::*;

impl NavidromeApp {
    /// Today's first four covers; missing slots show the default cover.
    fn render_daily_mix_mosaic(&self, size: f32) -> gpui_kit::AnyElement {
        let gap = 4.0;
        let cell = (size - gap) / 2.0;
        let covers = self
            .state
            .daily_mix
            .snapshot()
            .map(|snapshot| {
                snapshot
                    .tracks
                    .iter()
                    .filter_map(|track| track.song.cover_art.as_deref())
                    .take(4)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        div()
            .flex_none()
            .w(px(size))
            .h(px(size))
            .grid()
            .grid_cols(2)
            .gap(px(gap))
            .children((0..4).map(|index| {
                self.render_cover_styled(
                    covers.get(index).copied(),
                    cell,
                    tokens::radius::CONTROL,
                    Vec::new(),
                )
            }))
            .into_any_element()
    }

    pub(in crate::app) fn render_daily_mix_card(
        &self,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let loading = self.state.daily_mix.loading();
        let error = self.state.daily_mix.error();
        let count = self
            .state
            .daily_mix
            .snapshot()
            .map_or(0, |snapshot| snapshot.tracks.len());
        let description = if loading {
            t!("Creating your daily mix…")
        } else if let Some(error) = error {
            t!("Daily mix unavailable: %{error}", error = error)
        } else {
            t!("A balanced mix of favorites, familiar artists, and new discoveries.")
        };
        let title = if count > 0 {
            t!("%{count} tracks selected for today", count = count)
        } else {
            t!("Daily Mix")
        };
        raised_surface(
            h_flex()
                .w_full()
                .h(px(if compact {
                    tokens::HERO_CARD_COMPACT_HEIGHT
                } else {
                    tokens::HERO_CARD_HEIGHT
                }))
                .when(compact, |this| this.p_4().gap_4())
                .when(!compact, |this| this.p_6().gap_6())
                .items_center(),
            tokens::radius::LARGE,
            cx,
        )
        .child(self.render_daily_mix_mosaic(if compact {
            tokens::HERO_COVER_COMPACT
        } else {
            tokens::HERO_COVER
        }))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .when(count > 0, |this| {
                    this.child(
                        div()
                            .text_size(px(tokens::META))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text_secondary)
                            .child(t!("Daily Mix")),
                    )
                })
                .child(
                    serif_heading(
                        title,
                        if compact {
                            tokens::CARD_TITLE_COMPACT
                        } else {
                            tokens::CARD_TITLE
                        },
                        cx,
                    )
                    .truncate(),
                )
                .child(
                    div()
                        .text_size(px(tokens::META))
                        .truncate()
                        .text_color(if error.is_some() {
                            cx.theme().danger
                        } else {
                            p.text_secondary
                        })
                        .child(description),
                )
                .child(
                    h_flex()
                        .pt_2()
                        .gap_2()
                        .items_center()
                        .when(count == 0, |this| {
                            this.child(
                                Button::new("daily-mix-create")
                                    .label(if loading {
                                        t!("Creating…")
                                    } else if error.is_some() {
                                        t!("Retry")
                                    } else {
                                        t!("Create and play")
                                    })
                                    .primary()
                                    .rounded(px(tokens::radius::FULL))
                                    .small()
                                    .disabled(loading)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if this.state.daily_mix.error().is_some() {
                                            this.retry_daily_mix();
                                        } else {
                                            this.play_daily_mix();
                                        }
                                        cx.notify();
                                    })),
                            )
                        })
                        .when(count > 0, |this| {
                            this.child(
                                round_play_button(
                                    "daily-mix-play",
                                    PlayerIcon::Play,
                                    t!("Play"),
                                    if compact {
                                        tokens::PLAY_BUTTON_COMPACT
                                    } else {
                                        tokens::PLAY_BUTTON
                                    },
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.play_daily_mix();
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                capsule_button("daily-mix-append")
                                    .icon(AppIcon::Queue)
                                    .tooltip(t!("Add to queue"))
                                    .small()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.append_daily_mix();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("daily-mix-refresh")
                                    .icon(AppIcon::Refresh)
                                    .tooltip(t!("Refresh mix"))
                                    .ghost()
                                    .small()
                                    .rounded(px(tokens::radius::CONTROL))
                                    .disabled(loading)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.refresh_daily_mix();
                                        cx.notify();
                                    })),
                            )
                        }),
                ),
        )
        .into_any_element()
    }
}
