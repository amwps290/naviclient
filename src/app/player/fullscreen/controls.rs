//! Full-width progress and transport controls for presentation screens.

use super::layout::FullscreenLayout;
use super::view::{salon_button, salon_palette, salon_toggle_style};
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
        let status = h_flex()
            .min_w_0()
            .gap_2()
            .items_center()
            .text_size(px(10.0))
            .child(div().size(px(4.0)).rounded_full().bg(p.accent))
            .child(if playback.error.is_some() {
                t!("Playback error")
            } else if playback.paused {
                t!("Paused")
            } else if playback.stall.is_some() || playback.active && !playback.output_connected {
                t!("Buffering")
            } else if !playback.active {
                t!("Ready to play")
            } else {
                t!("Playing")
            });

        v_flex()
            .w_full()
            .h_full()
            .gap(px(if layout.compact { 10.0 } else { 14.0 }))
            .text_color(p.secondary)
            .track_focus(&self.fullscreen.footer_focus)
            .child(self.render_player_progress_styled(&playback, p.accent, p.muted, None, cx))
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
            )
            .when(!layout.stacked, |this| {
                this.child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .items_center()
                        .text_size(px(9.0))
                        .text_color(p.muted.opacity(0.65))
                        .child(t!("Vinyl Salon"))
                        .child(t!("Esc to exit · H controls · T translation · V waveform")),
                )
            })
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
        h_flex()
            .items_center()
            .gap(px(if compact { 15.0 } else { 22.0 }))
            .flex_none()
            .child(
                self.shuffle_button(cx)
                    .custom(salon_toggle_style(self.state.playback_modes.shuffle, cx)),
            )
            .child(
                Button::new("fullscreen-previous")
                    .icon(PlayerIcon::Previous)
                    .custom(salon_toggle_style(false, cx))
                    .with_size(px(36.0))
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
                            .hover(p.accent)
                            .active(p.accent),
                    )
                    .selected(true)
                    .with_size(px(if compact { 48.0 } else { 55.0 }))
                    .rounded_full()
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
                    .with_size(px(36.0))
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
                            .w(px(if compact { 64.0 } else { 86.0 }))
                            .bg(p.secondary)
                            .text_color(p.secondary)
                            .rounded_full(),
                    ),
            )
            .into_any_element()
    }
}
