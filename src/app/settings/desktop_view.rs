//! Desktop notification and window behavior preferences.

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn render_desktop_settings(
        &self,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        v_flex()
            .gap_3()
            .child(section_title(t!("System integration"), cx))
            .child(
                v_flex()
                    .gap_4()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_4()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .child(t!("Now-playing notifications")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(
                                                t!("Send a desktop notification with the cover, title, artist and album when a track starts. Rapid switching replaces the previous notification."
                                                ),
                                            ),
                                    ),
                            )
                            .child(
                                Switch::new("track-notifications-switch")
                                    .checked(self.config.show_track_notifications)
                                    .accessibility_label(t!("Now-playing notifications"))
                                    .on_change(cx.listener(|this, checked, _, cx| {
                                        this.set_show_track_notifications(*checked, cx);
                                    })),
                            ),
                    )
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_4()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .child(t!("Close button minimizes")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(
                                                t!("The window close button minimizes to the taskbar instead of quitting; tray Exit still quits. Some Wayland compositors ignore minimize requests."
                                                ),
                                            ),
                                    ),
                            )
                            .child(
                                Switch::new("close-minimizes-switch")
                                    .checked(self.config.close_minimizes)
                                    .accessibility_label(t!("Close button minimizes"))
                                    .on_change(cx.listener(|this, checked, _, cx| {
                                        this.set_close_minimizes(*checked, cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }
}
