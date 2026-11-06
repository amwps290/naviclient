//! Playback progress, buffered range, stall feedback and retry errors.

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn render_player_progress(
        &self,
        playback: &PlaybackState,
        accent: Hsla,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let duration = playback.duration.unwrap_or_default();
        let buffered_percent = if duration.is_zero() {
            0.0
        } else {
            (playback.buffered.as_secs_f32() / duration.as_secs_f32() * 100.0).clamp(0.0, 100.0)
        };
        h_flex()
            .w_full()
            .min_w_0()
            .h(px(24.0))
            .max_w(px(760.0))
            .items_center()
            .gap_2()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .when_some(playback.stall, |this, stall| {
                this.child(render_stall_chip(stall, accent))
            })
            .when_some(playback.error.clone(), |this, error| {
                this.child(render_playback_error_chip(error, cx))
            })
            .child(
                div()
                    .flex_none()
                    .min_w(px(40.0))
                    .text_right()
                    .child(format_playback(playback.position)),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .mx_1()
                    .h_6()
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px(9.0))
                            .h_1p5()
                            .w(relative(buffered_percent / 100.0))
                            .rounded_full()
                            .bg(accent.opacity(0.24)),
                    )
                    .child(
                        Slider::new(&self.playback_slider)
                            .w_full()
                            .bg(accent)
                            .text_color(accent)
                            .rounded_full(),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .min_w(px(40.0))
                    .child(format_playback(duration)),
            )
            .into_any_element()
    }
}

/// 播放中数据停止到达时的紧凑提示：转圈图标 + 已等待秒数。
/// 目的不是报错，而是让用户立刻分清「在等网络」与「卡死」。
fn render_stall_chip(stall: Duration, accent: Hsla) -> gpui_kit::AnyElement {
    let seconds = stall.as_secs();
    let label = if seconds >= 2 {
        t!("Buffering %{seconds}s", seconds = seconds).to_string()
    } else {
        t!("Buffering").to_string()
    };
    h_flex()
        .id("player-stall")
        .flex_none()
        .h(px(20.0))
        .px_1p5()
        .gap_1()
        .items_center()
        .rounded(px(4.0))
        .bg(accent.opacity(0.10))
        .text_size(px(12.0))
        .text_color(accent)
        .child(Icon::new(KitIcon::LoaderCircle).with_size(px(11.0)))
        .child(label)
        .into_any_element()
}

/// 播放失败的紧凑提示 + 立即重试。
/// 播放条只有 24px 高，这里只显示归因摘要，完整原因放在悬停提示里。
fn render_playback_error_chip(error: String, cx: &Context<NavidromeApp>) -> gpui_kit::AnyElement {
    let tooltip = error.clone();
    let summary = error
        .split_once(": ")
        .map_or(error.as_str(), |(summary, _)| summary)
        .to_string();
    h_flex()
        .id("player-error")
        .flex_none()
        .h(px(20.0))
        .gap_1()
        .items_center()
        .text_size(px(12.0))
        .text_color(cx.theme().danger)
        .child(Icon::new(KitIcon::TriangleAlert).with_size(px(11.0)))
        .child(
            div()
                .id("player-error-text")
                .max_w(px(150.0))
                .truncate()
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .child(summary),
        )
        .child(
            Button::new("player-retry")
                .label(t!("Retry"))
                .ghost()
                .xsmall()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.retry_current_track();
                    cx.notify();
                })),
        )
        .into_any_element()
}
