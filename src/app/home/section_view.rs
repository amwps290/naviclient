//! Home album-section headers, previews and collapse interaction.

use crate::app::prelude::*;
use crate::app::widgets::shelf::shelf_section;

impl NavidromeApp {
    /// Build album content only while expanded; hidden grids must not create
    /// cover elements or register row callbacks during a Home render.
    pub(in crate::app) fn render_home_section(
        &self,
        id: &'static str,
        title: impl Into<SharedString>,
        render_content: impl FnOnce() -> gpui_kit::AnyElement,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let title: SharedString = title.into();
        let collapsed = self.state.collapsed_sections.contains(id);
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .id(SharedString::from(format!("home-section-{id}")))
                    .hover(|style| style.text_color(cx.theme().primary))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_home_section(id, cx);
                    }))
                    .child(section_title(title.clone(), cx))
                    .child(
                        Icon::new(if collapsed {
                            AppIcon::ChevronRight
                        } else {
                            AppIcon::ChevronDown
                        })
                        .size(px(15.0))
                        .text_color(palette(cx).text_muted),
                    ),
            )
            .when(!collapsed, |this| this.child(render_content()))
            .into_any_element()
    }

    pub(in crate::app) fn render_home_primary_section(
        &self,
        title: impl Into<SharedString>,
        content: gpui_kit::AnyElement,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        shelf_section(section_title(title, cx).into_any_element(), content)
    }

    /// Home 页专辑区块的通用内容：加载骨架 / 空状态提示 / 专辑网格。
    pub(in crate::app) fn render_home_album_content(
        &self,
        scope: &str,
        albums: &[Album],
        loading: bool,
        empty_text: impl Into<SharedString>,
        cx: &Context<Self>,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        let empty_text: SharedString = empty_text.into();
        if albums.is_empty() && loading {
            self.render_album_skeleton_grid(window, cx)
        } else if albums.is_empty() {
            v_flex()
                .w_full()
                .min_h(px(132.0))
                .items_center()
                .justify_center()
                .gap_2()
                .text_center()
                .child(
                    Icon::new(AppIcon::Albums)
                        .with_size(px(28.0))
                        .text_color(cx.theme().muted_foreground.opacity(0.72)),
                )
                .child(
                    div()
                        .max_w(px(420.0))
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(empty_text),
                )
                .into_any_element()
        } else {
            self.render_album_grid(scope, albums, cx, window)
        }
    }

    pub(in crate::app) fn toggle_home_section(&mut self, id: &'static str, cx: &mut Context<Self>) {
        if self.state.collapsed_sections.contains(id) {
            self.state.collapsed_sections.remove(id);
        } else {
            self.state.collapsed_sections.insert(id.to_string());
        }
        cx.notify();
    }
}
