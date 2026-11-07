//! Brand, capsule and toggle button recipes shared by the whole app.

use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::{px, App, ElementId, SharedString, Styled};

use super::tokens::{palette, radius};
use crate::assets::PlayerIcon;

// The kit paints a `Custom` variant at 20% opacity until hovered, so solid
// buttons use its `primary` and `secondary` variants, whose colors come from
// the theme overrides. Toggles pair `Custom` with `selected`, which paints the
// `active` color in full.

/// Quiet while off; brand color only while the mode is on.
pub(in crate::app) fn soft_toggle_style(active: bool, cx: &App) -> ButtonCustomVariant {
    let p = palette(cx);
    if active {
        ButtonCustomVariant::new(cx)
            .color(p.brand_soft)
            .foreground(p.brand)
            .hover(p.brand_soft)
            .active(p.brand_soft)
    } else {
        ButtonCustomVariant::new(cx)
            .color(p.page.opacity(0.0))
            .foreground(p.text_secondary)
            .hover(p.sunken)
            .active(p.border)
    }
}

pub(in crate::app) fn round_play_button(
    id: impl Into<ElementId>,
    icon: PlayerIcon,
    label: impl Into<SharedString>,
    size: f32,
) -> Button {
    let label = label.into();
    Button::new(id)
        .icon(icon)
        .accessibility_label(label.clone())
        .tooltip(label)
        .primary()
        .rounded(px(radius::FULL))
        .w(px(size))
        .h(px(size))
}

pub(in crate::app) fn capsule_button(id: impl Into<ElementId>) -> Button {
    Button::new(id).secondary().rounded(px(radius::FULL))
}
