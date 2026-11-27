//! Main-window and now-playing titlebars and window actions.

use crate::app::prelude::*;
use crate::app::shortcuts::bindings::shortcut_display;

impl NavidromeApp {
    fn render_titlebar_actions(
        &self,
        mini_id: &'static str,
        settings_id: &'static str,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        h_flex()
            .flex_none()
            .items_center()
            .gap_1()
            .occlude()
            .child(
                icon_button(
                    SharedString::from(format!("{mini_id}-fullscreen")),
                    AppIcon::Fullscreen,
                    t!(
                        "Fullscreen player (%{shortcut})",
                        shortcut = shortcut_display(
                            self.config
                                .shortcuts
                                .binding(ShortcutAction::ToggleFullscreen)
                        )
                    ),
                    tokens::ICON_BUTTON_XS,
                )
                .accessibility_label(t!("Fullscreen player"))
                .on_click(
                    cx.listener(|this, _, window, cx| this.toggle_fullscreen_player(window, cx)),
                ),
            )
            .child(
                icon_button(
                    mini_id,
                    AppIcon::MiniPlayer,
                    t!("Switch to mini player"),
                    tokens::ICON_BUTTON_XS,
                )
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_mini_mode(window, cx);
                })),
            )
            .child(
                toggle_icon_button(
                    settings_id,
                    AppIcon::Settings,
                    t!("Settings"),
                    self.state.settings.open,
                    tokens::ICON_BUTTON_XS,
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.state.settings.open {
                        this.state.settings.close();
                    } else {
                        this.state.settings.open = true;
                    }
                    if this.state.settings.open {
                        this.refresh_output_devices(cx);
                    } else {
                        this.cancel_shortcut_recording();
                    }
                    cx.notify();
                })),
            )
            .into_any_element()
    }

    /// 主窗口标题栏：左侧为浏览历史后退/前进按钮，右侧为窗口级工具。
    pub(in crate::app) fn render_main_titlebar(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let can_back =
            self.state.nav_history.can_go_back() || self.state.view.detail_parent().is_some();
        let can_forward = self.state.nav_history.can_go_forward();
        let detail_parent = self.state.view.detail_parent();

        h_flex()
            .h_full()
            .w_full()
            .items_center()
            .justify_between()
            .pr_2()
            .child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .gap_0p5()
                    .occlude()
                    .child(
                        icon_button(
                            "titlebar-back",
                            KitIcon::ChevronLeft,
                            t!("Back (Alt+Left)"),
                            tokens::ICON_BUTTON_XS,
                        )
                        .disabled(!can_back)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.navigate_back() {
                                if let Some(parent) = detail_parent {
                                    this.navigate_to(parent);
                                }
                            }
                            cx.notify();
                        })),
                    )
                    .child(
                        icon_button(
                            "titlebar-forward",
                            AppIcon::ChevronRight,
                            t!("Forward (Alt+Right)"),
                            tokens::ICON_BUTTON_XS,
                        )
                        .disabled(!can_forward)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.navigate_forward();
                            cx.notify();
                        })),
                    ),
            )
            .child(self.render_titlebar_actions("toggle-mini-player", "title-settings", cx))
            .into_any_element()
    }

    pub(in crate::app) fn render_now_playing_titlebar(
        &self,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let track_label = self
            .state
            .now_playing
            .as_ref()
            .map(|song| {
                if song.artist.trim().is_empty() {
                    song.title.clone()
                } else {
                    format!("{} · {}", song.title, song.artist)
                }
            })
            .unwrap_or_else(|| t!("Now Playing").to_string());

        h_flex()
            .h_full()
            .w_full()
            .items_center()
            .pr_2()
            .child(
                h_flex()
                    .w(px(190.0))
                    .flex_none()
                    .items_center()
                    .gap_1()
                    .occlude()
                    .child(
                        icon_button(
                            "leave-now-playing",
                            AppIcon::ArrowLeft,
                            t!("Back to library"),
                            tokens::ICON_BUTTON_XS,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.leave_now_playing();
                            cx.notify();
                        })),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t!("Now Playing")),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px_4()
                    .text_center()
                    .text_size(px(tokens::CAPTION))
                    .text_color(palette(cx).text_secondary)
                    .truncate()
                    .child(track_label),
            )
            .child(self.render_titlebar_actions(
                "now-playing-mini-player",
                "now-playing-settings",
                cx,
            ))
            .into_any_element()
    }
}
