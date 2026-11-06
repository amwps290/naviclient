//! 设置覆盖层及其分区导航。

use super::*;

impl NavidromeApp {
    /// 设置覆盖层：盖住侧边栏与内容区，点击背景或 Esc 关闭；
    /// 标题栏和底部播放条不被遮挡，改设置时也能控制播放。
    pub(super) fn render_settings_overlay(
        &self,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let settings_content = div()
            .id("settings-content")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.settings_scroll)
            .vertical_scrollbar(&self.settings_scroll)
            .when(compact, |this| this.px_5().py_4())
            .when(!compact, |this| this.px_6().py_5())
            .child(self.render_settings(cx));
        let settings_body = if compact {
            v_flex()
                .id("settings-body")
                .flex_1()
                .min_h_0()
                .items_stretch()
                .child(self.render_settings_navigation(true, cx))
                .child(settings_content)
                .into_any_element()
        } else {
            h_flex()
                .id("settings-body")
                .flex_1()
                .min_h_0()
                .items_stretch()
                .child(self.render_settings_navigation(false, cx))
                .child(settings_content)
                .into_any_element()
        };

        div()
            .id("settings-backdrop")
            .absolute()
            .inset_0()
            .occlude() // 遮罩挡住背后所有命中框：后方页面不再响应滚轮/悬停
            .flex()
            .justify_center()
            .items_start()
            // 固定黑色半透明背景：亮/暗主题下都是压暗效果。
            .bg(hsla(0.0, 0.0, 0.0, 0.35))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.state.settings.open = false;
                    this.cancel_shortcut_recording();
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .id("settings-card")
                    .w(px(960.0))
                    .h(px(650.0))
                    .max_w(relative(if compact { 0.96 } else { 0.94 }))
                    .max_h(relative(if compact { 0.94 } else { 0.88 }))
                    .mt(px(if compact { 16.0 } else { 48.0 }))
                    .bg(cx.theme().background)
                    .rounded_xl()
                    .shadow_xl()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        // 点击卡片内部不触发背景关闭。
                        cx.stop_propagation()
                    })
                    .child(
                        h_flex()
                            .flex_none()
                            .when(compact, |this| this.px_4().py_2())
                            .when(!compact, |this| this.px_5().py_3())
                            .justify_between()
                            .items_center()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("Settings")),
                            )
                            .child(
                                Button::new("close-settings")
                                    .icon(AppIcon::Close)
                                    .tooltip(t!("Close settings"))
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.state.settings.open = false;
                                        this.cancel_shortcut_recording();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(settings_body),
            )
            .into_any_element()
    }
}
