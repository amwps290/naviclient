//! Player volume button, hover panel and drag event boundaries.

use crate::app::prelude::*;
use crate::app::shortcuts::bindings::shortcut_display;

impl NavidromeApp {
    pub(in crate::app) fn render_volume_control(
        &self,
        accent: Hsla,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let output_volume = self.volume.output_volume(self.config.volume);
        let volume_icon = if self.volume.muted() || output_volume <= 0.001 {
            AppIcon::VolumeMuted
        } else if output_volume < 0.5 {
            AppIcon::VolumeLow
        } else {
            AppIcon::VolumeHigh
        };
        let volume_value = match self.volume_slider.read(cx).value() {
            SliderValue::Single(value) => value,
            SliderValue::Range(_, value) => value,
        };
        div()
            .id("player-volume-control")
            .relative()
            .flex_none()
            .on_hover(cx.listener(|this, hovering, _, cx| {
                if *hovering {
                    this.show_volume_panel(cx);
                } else {
                    this.schedule_volume_panel_close(cx);
                }
            }))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                this.adjust_volume_from_scroll(event, window, cx);
            }))
            .when(self.volume.panel_open(), |this| {
                this.child(
                    v_flex()
                        .id("player-volume-panel")
                        .absolute()
                        .bottom(px(64.0))
                        .left(px(-13.0))
                        .w(px(54.0))
                        .p_2()
                        .items_center()
                        .gap_1()
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().popover)
                        .text_color(cx.theme().popover_foreground)
                        .shadow_md()
                        .capture_any_mouse_down(cx.listener(|this, _, _, cx| {
                            this.begin_volume_panel_drag(cx);
                        }))
                        .capture_any_mouse_up(cx.listener(|this, _, _, cx| {
                            this.end_volume_panel_drag(true, cx);
                        }))
                        .on_mouse_up_out(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.end_volume_panel_drag(false, cx);
                            }),
                        )
                        .on_hover(cx.listener(|this, hovering, _, cx| {
                            if *hovering {
                                this.show_volume_panel(cx);
                            } else {
                                this.schedule_volume_panel_close(cx);
                            }
                        }))
                        .child(
                            div()
                                .h(px(18.0))
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(if self.volume.muted() {
                                    cx.theme().muted_foreground
                                } else {
                                    cx.theme().popover_foreground
                                })
                                .child(format!("{}%", volume_value.round() as u32)),
                        )
                        .child(
                            div()
                                .h(px(120.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    Slider::new(&self.volume_slider)
                                        .vertical()
                                        .h(px(120.0))
                                        .bg(accent)
                                        .text_color(accent)
                                        .rounded_full(),
                                ),
                        ),
                )
            })
            .child(
                Button::new("player-volume-mute")
                    .icon(volume_icon)
                    .tooltip(if self.volume.muted() {
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
                    })
                    .ghost()
                    .with_size(px(30.0))
                    .rounded_full()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_mute(window, cx);
                    })),
            )
            .into_any_element()
    }
}
