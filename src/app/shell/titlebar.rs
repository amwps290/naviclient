//! Main-window and now-playing titlebars and window actions.

use crate::app::prelude::*;

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
                Button::new(mini_id)
                    .icon(AppIcon::MiniPlayer)
                    .tooltip(t!("Switch to mini player"))
                    .ghost()
                    .small()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_mini_mode(window, cx);
                    })),
            )
            .child(
                Button::new(settings_id)
                    .icon(AppIcon::Settings)
                    .tooltip(t!("Settings"))
                    .ghost()
                    .small()
                    .selected(self.state.settings.open)
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

    /// 主窗口标题栏：页面名称由内容区标题承担，这里只留详情页的返回入口
    /// 和窗口级工具。
    pub(in crate::app) fn render_main_titlebar(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let detail_parent = self.state.view.detail_parent();

        h_flex()
            .h_full()
            .w_full()
            .items_center()
            .justify_between()
            .pr_2()
            .child(div().min_w_0().when_some(detail_parent, |this, parent| {
                this.child(
                    div().occlude().child(
                        Button::new("titlebar-back")
                            .icon(AppIcon::ArrowLeft)
                            .tooltip(t!("Back to library"))
                            .ghost()
                            .small()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.state.navigate_to(parent);
                                cx.notify();
                            })),
                    ),
                )
            }))
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
                        Button::new("leave-now-playing")
                            .icon(AppIcon::ArrowLeft)
                            .tooltip(t!("Back to library"))
                            .ghost()
                            .small()
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
