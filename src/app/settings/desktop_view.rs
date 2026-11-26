//! Desktop notification and window behavior preferences.

use super::components::{settings_group, settings_row};
use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn render_desktop_settings(
        &self,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        settings_group(
            Some(t!("System integration").into()),
            None,
            vec![
                settings_row(
                    t!("Now-playing notifications"),
                    Some(
                        t!("Send a desktop notification with the cover, title, artist and album when a track starts. Rapid switching replaces the previous notification.")
                            .into(),
                    ),
                    Switch::new("track-notifications-switch")
                        .checked(self.config.show_track_notifications)
                        .accessibility_label(t!("Now-playing notifications"))
                        .on_change(cx.listener(|this, checked, _, cx| {
                            this.set_show_track_notifications(*checked, cx);
                        }))
                        .into_any_element(),
                    cx,
                ),
                settings_row(
                    t!("Close button minimizes"),
                    Some(
                        t!("The window close button minimizes to the taskbar instead of quitting; tray Exit still quits. Some Wayland compositors ignore minimize requests.")
                            .into(),
                    ),
                    Switch::new("close-minimizes-switch")
                        .checked(self.config.close_minimizes)
                        .accessibility_label(t!("Close button minimizes"))
                        .on_change(cx.listener(|this, checked, _, cx| {
                            this.set_close_minimizes(*checked, cx);
                        }))
                        .into_any_element(),
                    cx,
                ),
            ],
            cx,
        )
    }
}
