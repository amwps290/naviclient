//! 设置覆盖层及其分区导航。

use crate::app::prelude::*;

impl NavidromeApp {
    /// 设置覆盖层：盖住侧边栏与内容区，点击背景或 Esc 关闭；
    /// 标题栏和底部播放条不被遮挡，改设置时也能控制播放。
    pub(in crate::app) fn render_settings_overlay(
        &self,
        compact: bool,
        player_clearance: f32,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let dark = cx.theme().is_dark();
        let hairline = if dark {
            hsla(0.0, 0.0, 1.0, 0.07)
        } else {
            p.border.opacity(0.70)
        };
        let rail_bg = if dark {
            p.sunken.opacity(0.65)
        } else {
            p.page.opacity(0.75)
        };

        let settings_content = div()
            .id("settings-content")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.settings_scroll)
            .vertical_scrollbar(&self.settings_scroll)
            .when(compact, |this| this.px_4().py_4())
            .when(!compact, |this| this.px_7().py_6())
            .child(self.render_settings(cx));

        let card_body = if compact {
            v_flex()
                .id("settings-body")
                .size_full()
                .min_h_0()
                .items_stretch()
                .child(
                    h_flex()
                        .flex_none()
                        .px_4()
                        .py_3()
                        .border_b_1()
                        .border_color(hairline)
                        .justify_between()
                        .items_center()
                        .child(section_title(t!("Settings"), cx))
                        .child(self.render_settings_close_button(cx)),
                )
                .child(
                    div()
                        .flex_none()
                        .bg(rail_bg)
                        .border_b_1()
                        .border_color(hairline)
                        .child(self.render_settings_navigation(true, cx)),
                )
                .child(settings_content)
                .into_any_element()
        } else {
            h_flex()
                .id("settings-body")
                .size_full()
                .min_h_0()
                .items_stretch()
                .child(
                    v_flex()
                        .w(px(216.0))
                        .h_full()
                        .flex_none()
                        .bg(rail_bg)
                        .border_r_1()
                        .border_color(hairline)
                        .child(
                            h_flex()
                                .flex_none()
                                .px_5()
                                .pt_5()
                                .pb_3()
                                .items_center()
                                .gap_2()
                                .child(
                                    Icon::new(AppIcon::Settings)
                                        .with_size(px(16.0))
                                        .text_color(p.text_muted),
                                )
                                .child(section_title(t!("Settings"), cx)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_h_0()
                                .child(self.render_settings_navigation(false, cx)),
                        ),
                )
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .child(settings_content)
                        .child(
                            div()
                                .absolute()
                                .top_4()
                                .right_4()
                                .child(self.render_settings_close_button(cx)),
                        ),
                )
                .into_any_element()
        };

        div()
            .id("settings-backdrop")
            .absolute()
            .inset_0()
            .pb(px(player_clearance))
            .pt(px(if compact { 12.0 } else { 20.0 }))
            .px(px(if compact { 10.0 } else { 24.0 }))
            .occlude() // 遮罩挡住背后所有命中框：后方页面不再响应滚轮/悬停
            .flex()
            .justify_center()
            .items_center()
            .bg(tokens::scrim(cx))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.state.settings.close();
                    this.cancel_shortcut_recording();
                    cx.notify();
                }),
            )
            .child(
                translucent_surface(
                    v_flex()
                        .id("settings-card")
                        .w(px(960.0))
                        .h(px(660.0))
                        .max_w(relative(if compact { 0.98 } else { 0.94 }))
                        .max_h(relative(if compact { 0.96 } else { 0.92 }))
                        .overflow_hidden(),
                    tokens::radius::LARGE,
                    self.now_playing_cover_palette().map(|(base, _)| base),
                    cx,
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                    // 点击卡片内部不触发背景关闭。
                    cx.stop_propagation()
                })
                .child(card_body),
            )
            .into_any_element()
    }

    fn render_settings_close_button(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        Button::new("close-settings")
            .icon(AppIcon::Close)
            .tooltip(t!("Close settings"))
            .ghost()
            .small()
            .rounded_full()
            .on_click(cx.listener(|this, _, _, cx| {
                this.state.settings.close();
                this.cancel_shortcut_recording();
                cx.notify();
            }))
            .into_any_element()
    }
}
