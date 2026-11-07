//! Serif heading styles. Body text keeps the kit's UI font.

use gpui_kit::{div, px, relative, App, Div, FontWeight, ParentElement, SharedString, Styled};

use crate::typography::serif_font_family;

pub(in crate::app) fn serif_heading(text: impl Into<SharedString>, size: f32, cx: &App) -> Div {
    div()
        .font_family(serif_font_family(cx))
        .font_weight(FontWeight::BOLD)
        .text_size(px(size))
        .line_height(relative(1.2))
        .child(text.into())
}

pub(in crate::app) fn section_title(text: impl Into<SharedString>, cx: &App) -> Div {
    serif_heading(text, super::tokens::SECTION_TITLE, cx)
}
