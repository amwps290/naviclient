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
                        this.state.settings.open = !this.state.settings.open;
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

    /// 主窗口标题栏：左侧呈现页面上下文，右侧保留窗口级工具；
    /// 详情页提供返回其所属媒体库分区的明确入口。
    pub(in crate::app) fn render_main_titlebar(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let detail_parent = self.state.view.detail_parent();
        let (section_label, section_icon) = self.state.view.navigation_presentation();

        h_flex()
            .h_full()
            .w_full()
            .items_center()
            .pr_2()
            .child(
                h_flex()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap_2()
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .gap_1()
                            .when_some(detail_parent, |this, parent| {
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
                            })
                            .child(
                                section_icon
                                    .with_size(px(14.0))
                                    .text_color(cx.theme().muted_foreground.opacity(0.86)),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(cx.theme().muted_foreground)
                                    .child(section_label),
                            ),
                    )
                    .child(div().flex_none().flex().justify_end().child(
                        self.render_titlebar_actions("toggle-mini-player", "title-settings", cx),
                    )),
            )
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
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(cx.theme().muted_foreground)
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
