//! Home Daily Mix card and its playback, refresh and append controls.

use crate::app::covers::artwork::{accent_foreground, adjust_lightness};
use crate::app::prelude::*;
use crate::app::shell::view_helpers::compact_window_layout;

impl NavidromeApp {
    pub(in crate::app) fn render_daily_mix_card(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let compact = compact_window_layout(f32::from(window.viewport_size().width));
        let feature_height = if compact { 144.0 } else { 176.0 };
        let count = self
            .state
            .daily_mix
            .snapshot()
            .map_or(0, |snapshot| snapshot.tracks.len());
        let subtitle = if self.state.daily_mix.loading() {
            t!("Creating your daily mix…")
        } else if let Some(error) = self.state.daily_mix.error() {
            t!("Daily mix unavailable: %{error}", error = error)
        } else if count > 0 {
            t!("%{count} tracks selected for today", count = count)
        } else {
            t!("A balanced mix of favorites, familiar artists, and new discoveries.")
        };
        let mix_cover = self.state.daily_mix.snapshot().and_then(|snapshot| {
            snapshot
                .tracks
                .iter()
                .find_map(|track| track.song.cover_art.as_deref())
        });
        let (surface_start, surface_end, accent) = self.home_feature_colors(mix_cover, cx);
        let play_style = ButtonCustomVariant::new(cx)
            .color(accent)
            .foreground(accent_foreground(accent))
            .hover(adjust_lightness(accent, 0.05))
            .active(adjust_lightness(accent, -0.05));
        v_flex()
            .w_full()
            .h(px(feature_height))
            .justify_between()
            .gap_3()
            .rounded(px(12.0))
            .border_1()
            .border_color(cx.theme().border.opacity(0.58))
            .shadow_sm()
            .bg(linear_gradient(
                118.0,
                linear_color_stop(surface_start, 0.0),
                linear_color_stop(surface_end, 1.0),
            ))
            .when(compact, |this| this.p_4())
            .when(!compact, |this| this.p_5())
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .size(px(if compact { 40.0 } else { 48.0 }))
                            .flex_none()
                            .rounded(px(8.0))
                            .bg(accent.opacity(0.12))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(AppIcon::Shuffle)
                                    .with_size(px(if compact { 18.0 } else { 22.0 }))
                                    .text_color(accent),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .when(compact, |this| this.text_base())
                                    .when(!compact, |this| this.text_lg())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("Daily Mix")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .truncate()
                                    .text_color(if self.state.daily_mix.error().is_some() {
                                        cx.theme().danger
                                    } else {
                                        cx.theme().foreground.opacity(0.72)
                                    })
                                    .child(subtitle),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_1()
                    .when(count == 0, |this| {
                        this.child(
                            Button::new("daily-mix-create")
                                .label(if self.state.daily_mix.loading() {
                                    t!("Creating…")
                                } else if self.state.daily_mix.error().is_some() {
                                    t!("Retry")
                                } else {
                                    t!("Create and play")
                                })
                                .custom(play_style)
                                .rounded(px(8.0))
                                .small()
                                .disabled(self.state.daily_mix.loading())
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
                            Button::new("daily-mix-play")
                                .icon(PlayerIcon::Play)
                                .accessibility_label(t!("Play"))
                                .tooltip(t!("Play"))
                                .custom(play_style)
                                .rounded(px(999.0))
                                .w(px(if compact { 40.0 } else { 44.0 }))
                                .h(px(if compact { 40.0 } else { 44.0 }))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.play_daily_mix();
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("daily-mix-append")
                                .icon(AppIcon::Queue)
                                .tooltip(t!("Add to queue"))
                                .ghost()
                                .small()
                                .rounded(px(8.0))
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
                                .rounded(px(8.0))
                                .disabled(self.state.daily_mix.loading())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.refresh_daily_mix();
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .into_any_element()
    }
}
