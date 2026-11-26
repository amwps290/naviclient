//! Floating glass dock with progress bar, audio format badge, transport, and volume controls.

use super::layout::FullscreenLayout;
use super::view::{salon_button, salon_palette, salon_toggle_style};
use crate::app::playback::display::{bilibili_technical_info, playback_technical_info};
use crate::app::prelude::*;
use crate::app::shortcuts::bindings::shortcut_display;

impl NavidromeApp {
    pub(super) fn render_fullscreen_footer(
        &self,
        layout: &FullscreenLayout,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = salon_palette();
        let playback = self.playback_for_display();
        let controls = self.render_fullscreen_transport(&playback, layout.compact, cx);
        let accessories = self.render_fullscreen_accessories(layout.compact, cx);
        let technical_info = self
            .state
            .now_playing
            .as_ref()
            .zip(self.state.now_playing_quality)
            .map(|(song, quality)| {
                if song.bilibili_part.is_some() {
                    bilibili_technical_info(self.state.now_playing_audio_quality.as_ref())
                } else {
                    playback_technical_info(song, quality)
                }
            });
        let status = h_flex()
            .min_w_0()
            .gap_2p5()
            .items_center()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .text_size(px(12.0))
                    .text_color(p.secondary)
                    .child(
                        div()
                            .size(px(6.0))
                            .rounded_full()
                            .bg(if playback.error.is_some() {
                                cx.theme().danger
                            } else if playback.active && !playback.paused {
                                p.accent
                            } else {
                                p.muted
                            }),
                    )
                    .child(if playback.error.is_some() {
                        t!("Playback error")
                    } else if playback.paused {
                        t!("Paused")
                    } else if playback.stall.is_some()
                        || playback.active && !playback.output_connected
                    {
                        t!("Buffering")
                    } else if !playback.active {
                        t!("Ready to play")
                    } else {
                        t!("Playing")
                    }),
            )
            .when_some(technical_info, |this, info| {
                let tooltip = info.tooltip;
                this.child(
                    div()
                        .id("fullscreen-format-badge")
                        .flex_none()
                        .px_2p5()
                        .py_0p5()
                        .rounded_full()
                        .bg(p.accent.opacity(0.11))
                        .border_1()
                        .border_color(p.accent.opacity(0.24))
                        .text_size(px(10.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.accent)
                        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                        .child(info.format),
                )
            });

        div()
            .w_full()
            .h_full()
            .flex()
            .items_end()
            .justify_center()
            .child(
                v_flex()
                    .w_full()
                    .max_w(px(960.0))
                    .px(px(if layout.compact { 18.0 } else { 24.0 }))
                    .py(px(if layout.compact { 10.0 } else { 14.0 }))
                    .gap(px(if layout.compact { 8.0 } else { 10.0 }))
                    .rounded(px(22.0))
                    .bg(linear_gradient(
                        180.0,
                        linear_color_stop(p.dock_top, 0.0),
                        linear_color_stop(p.dock_bottom, 1.0),
                    ))
                    .border_1()
                    .border_color(p.border)
                    .shadow(vec![box_shadow(
                        px(0.0),
                        px(16.0),
                        px(44.0),
                        px(-6.0),
                        hsla(0.0, 0.0, 0.0, 0.55),
                    )])
                    .text_color(p.secondary)
                    .track_focus(&self.fullscreen.footer_focus)
                    .child(
                        self.render_player_progress_styled(&playback, p.accent, p.muted, None, cx),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_4()
                            .when(layout.stacked, |this| this.flex_col().gap_2())
                            .when(!layout.stacked, |this| {
                                this.child(div().flex_1().min_w_0().child(status))
                            })
                            .child(controls)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .justify_end()
                                    .child(accessories),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_fullscreen_transport(
        &self,
        playback: &PlaybackState,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = salon_palette();
        let playing = playback.active && !playback.paused;
        let label = if playing { t!("Pause") } else { t!("Play") };
        let play_size = if compact { 42.0 } else { 46.0 };
        h_flex()
            .items_center()
            .gap(px(if compact { 12.0 } else { 18.0 }))
            .flex_none()
            .child(
                self.shuffle_button(cx)
                    .custom(salon_toggle_style(self.state.playback_modes.shuffle, cx)),
            )
            .child(
                Button::new("fullscreen-previous")
                    .icon(PlayerIcon::Previous)
                    .custom(salon_toggle_style(false, cx))
                    .with_size(px(34.0))
                    .rounded_full()
                    .accessibility_label(t!("Previous track"))
                    .tooltip(t!("Previous track"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.skip(-1);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("fullscreen-play-pause")
                    .icon(if playing {
                        PlayerIcon::Pause
                    } else {
                        PlayerIcon::Play
                    })
                    .custom(
                        ButtonCustomVariant::new(cx)
                            .color(p.accent)
                            .foreground(p.page)
                            .hover(p.active_lyric)
                            .active(p.accent.opacity(0.9)),
                    )
                    .selected(true)
                    .with_size(px(play_size))
                    .rounded_full()
                    .shadow(vec![box_shadow(
                        px(0.0),
                        px(6.0),
                        px(20.0),
                        px(-2.0),
                        p.accent.opacity(0.32),
                    )])
                    .accessibility_label(label.clone())
                    .tooltip(label)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_playback();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("fullscreen-next")
                    .icon(PlayerIcon::Next)
                    .custom(salon_toggle_style(false, cx))
                    .with_size(px(34.0))
                    .rounded_full()
                    .accessibility_label(t!("Next track"))
                    .tooltip(t!("Next track"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.skip(1);
                        cx.notify();
                    })),
            )
            .child(self.repeat_button(cx).custom(salon_toggle_style(
                self.state.playback_modes.repeat != RepeatMode::Off,
                cx,
            )))
            .into_any_element()
    }

    fn render_fullscreen_accessories(
        &self,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = salon_palette();
        let visualizer = self.fullscreen.state.visualizer_enabled;
        let volume = self.volume.output_volume(self.config.volume);
        let muted = self.volume.muted() || volume <= 0.001;
        let volume_icon = if muted {
            AppIcon::VolumeMuted
        } else if volume < 0.5 {
            AppIcon::VolumeLow
        } else {
            AppIcon::VolumeHigh
        };
        h_flex()
            .items_center()
            .gap_2()
            .flex_none()
            .child(
                salon_button(
                    "fullscreen-visualizer",
                    AppIcon::Waveform,
                    t!("Toggle waveform (V)"),
                    cx,
                )
                .custom(salon_toggle_style(visualizer, cx))
                .selected(visualizer)
                .toggled(visualizer)
                .on_click(cx.listener(|this, _, _, cx| this.toggle_fullscreen_visualizer(cx))),
            )
            .child(
                h_flex()
                    .id("fullscreen-volume")
                    .items_center()
                    .gap_1()
                    .on_scroll_wheel(cx.listener(|this, event, window, cx| {
                        this.adjust_volume_from_scroll(event, window, cx);
                    }))
                    .child(
                        salon_button(
                            "fullscreen-mute",
                            volume_icon,
                            if muted {
                                t!(
                                    "Unmute (%{shortcut})",
                                    shortcut = shortcut_display(
                                        self.config.shortcuts.binding(ShortcutAction::ToggleMute)
                                    )
                                )
                            } else {
                                t!(
                                    "Mute (%{shortcut})",
                                    shortcut = shortcut_display(
                                        self.config.shortcuts.binding(ShortcutAction::ToggleMute)
                                    )
                                )
                            },
                            cx,
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.toggle_mute(window, cx))),
                    )
                    .child(
                        Slider::new(&self.volume_slider)
                            .w(px(if compact { 68.0 } else { 90.0 }))
                            .bg(p.accent)
                            .text_color(p.accent)
                            .rounded_full(),
                    ),
            )
            .into_any_element()
    }
}
