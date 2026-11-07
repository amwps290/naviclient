//! Shared library page headers, refresh controls and retry rows.

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn page_header(
        &self,
        title: impl Into<SharedString>,
        subtitle: impl Into<SharedString>,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        v_flex()
            .gap_1()
            .child(serif_heading(title, tokens::PAGE_TITLE, cx))
            .child(
                div()
                    .text_size(px(tokens::META))
                    .text_color(palette(cx).text_secondary)
                    .child(subtitle.into()),
            )
            .into_any_element()
    }

    /// 页头右侧的单视图刷新按钮：只重载当前视图，不必全量刷新媒体库。
    pub(in crate::app) fn header_refresh_button(
        &self,
        id: &'static str,
        tooltip: impl Into<SharedString>,
        loading: bool,
        cx: &Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> gpui_kit::AnyElement {
        Button::new(id)
            .icon(AppIcon::Refresh)
            .tooltip(tooltip)
            .ghost()
            .small()
            .loading(loading)
            .on_click(cx.listener(move |this, _, _, cx| {
                action(this, cx);
                cx.notify();
            }))
            .into_any_element()
    }

    /// 列表底部的“加载失败重试”行：分页请求失败时可见，点击重新请求。
    pub(in crate::app) fn render_retry_row(
        &self,
        id: &'static str,
        cx: &Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> gpui_kit::AnyElement {
        div()
            .py_2()
            .flex()
            .justify_center()
            .child(
                Button::new(id)
                    .label(t!("Loading failed — try again"))
                    .outline()
                    .small()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        action(this, cx);
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}
