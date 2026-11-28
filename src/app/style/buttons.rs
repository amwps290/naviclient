//! Unified button recipes shared by the whole app.
//!
//! Two clear shape families keep visual hierarchy legible:
//! - **Media & listening controls** (play, transport, hero actions, filters,
//!   round icon buttons): circular or full-pill (`radius::FULL`).
//! - **Utility, settings & dialog controls** (pickers, forms, confirm/cancel,
//!   toolbar actions): soft 10px rounded rectangles (`radius::BUTTON`).

use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::component::{box_shadow, ActiveTheme, Disableable, Icon, Selectable, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    hsla, px, App, BoxShadow, ElementId, FontWeight, ParentElement, SharedString, Styled,
};

use super::tokens::{palette, radius};
use crate::assets::PlayerIcon;

// The kit paints a `Custom` variant at 20% opacity until hovered, so solid
// buttons use its `primary`, `secondary` and `default` variants, whose colors
// come from the theme overrides. Toggles pair `Custom` with `selected`, which
// paints the `active` color in full.

/// Quiet while off; brand soft tint while on.
pub(in crate::app) fn soft_toggle_style(active: bool, cx: &App) -> ButtonCustomVariant {
    let p = palette(cx);
    let dark = cx.theme().is_dark();
    if active {
        ButtonCustomVariant::new(cx)
            .color(p.brand_soft)
            .foreground(p.brand)
            .hover(p.brand_soft_hover)
            .active(p.brand_soft)
    } else {
        ButtonCustomVariant::new(cx)
            .color(p.page.opacity(0.0))
            .foreground(p.text_secondary)
            .hover(if dark {
                p.ghost_hover.opacity(0.55)
            } else {
                p.ghost_hover
            })
            .active(if dark {
                p.ghost_active.opacity(0.75)
            } else {
                p.ghost_active
            })
    }
}

fn play_button_shadow(cx: &App) -> Vec<BoxShadow> {
    let p = palette(cx);
    if cx.theme().is_dark() {
        vec![box_shadow(
            px(0.0),
            px(4.0),
            px(14.0),
            px(-2.0),
            p.brand.opacity(0.28),
        )]
    } else {
        vec![
            box_shadow(
                px(0.0),
                px(1.0),
                px(2.0),
                px(0.0),
                hsla(0.0, 0.0, 0.0, 0.08),
            ),
            box_shadow(px(0.0), px(4.0), px(12.0), px(-2.0), p.brand.opacity(0.30)),
        ]
    }
}

/// Comfortable proportions and consistent typography for labelled buttons.
pub(in crate::app) trait ButtonExt {
    /// Dense inline labelled button: 28px height, 10px horizontal padding.
    fn dense_action(self) -> Self;
    /// Inline / toolbar / settings labelled button: 34px height, 14px horizontal padding.
    fn compact_action(self) -> Self;
    /// Dialog footer / primary form button: 36px height, 18px horizontal padding.
    fn dialog_action(self) -> Self;
    /// Works around the kit's `Secondary` disabled background opacity (1.5 -> 1.0)
    /// so disabled tonal pills visually recede like every other control.
    fn disabled_tonal(self, disabled: bool) -> Self;
}

impl ButtonExt for Button {
    fn dense_action(self) -> Self {
        self.h(px(28.0))
            .px_2p5()
            .text_size(px(12.5))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
    }

    fn compact_action(self) -> Self {
        self.h(px(34.0))
            .px_3p5()
            .text_size(px(13.5))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
    }

    fn dialog_action(self) -> Self {
        self.h(px(36.0))
            .px(px(18.0))
            .text_size(px(14.0))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
    }

    fn disabled_tonal(self, disabled: bool) -> Self {
        self.disabled(disabled)
            .when(disabled, |this| this.opacity(0.5))
    }
}

/// Circular primary play/pause button with optical icon scaling (~43% of
/// diameter) and a soft brand-tinted shadow.
pub(in crate::app) fn round_play_button(
    id: impl Into<ElementId>,
    icon: PlayerIcon,
    label: impl Into<SharedString>,
    size: f32,
    cx: &App,
) -> Button {
    let label = label.into();
    let icon_size = (size * 0.43).clamp(13.0, 22.0);
    Button::new(id)
        .child(Icon::new(icon).with_size(px(icon_size)))
        .accessibility_label(label.clone())
        .tooltip(label)
        .primary()
        .rounded(px(radius::FULL))
        .w(px(size))
        .h(px(size))
        .p_0()
        .cursor_pointer()
        .shadow(play_button_shadow(cx))
}

/// Circular ghost icon button with balanced icon-to-circle proportion (16px in
/// 32px+ circles, 14px in compact 28-30px circles). Uses `.icon(...)` so
/// `.loading(true)` still swaps cleanly to a spinner.
pub(in crate::app) fn icon_button(
    id: impl Into<ElementId>,
    icon: impl Into<Icon>,
    label: impl Into<SharedString>,
    size: f32,
) -> Button {
    let label = label.into();
    let button = Button::new(id)
        .icon(icon.into())
        .accessibility_label(label.clone())
        .tooltip(label)
        .ghost()
        .rounded(px(radius::FULL))
        .cursor_pointer();
    let button = if size < 32.0 { button.small() } else { button };
    button.w(px(size)).h(px(size))
}

/// Circular toggle icon button for player mode switches (shuffle, repeat, EQ,
/// queue). Active state shows a soft brand tint with a subtle border and
/// tactile hover feedback.
pub(in crate::app) fn toggle_icon_button(
    id: impl Into<ElementId>,
    icon: impl Into<Icon>,
    label: impl Into<SharedString>,
    active: bool,
    size: f32,
    cx: &App,
) -> Button {
    let label = label.into();
    let p = palette(cx);
    let dark = cx.theme().is_dark();
    let button = Button::new(id)
        .icon(icon.into())
        .accessibility_label(label.clone())
        .tooltip(label)
        .custom(soft_toggle_style(active, cx))
        .selected(active)
        .toggled(active)
        .rounded(px(radius::FULL))
        .cursor_pointer();
    let button = if size < 32.0 { button.small() } else { button };
    button.w(px(size)).h(px(size)).when(active, |this| {
        this.border_1()
            .border_color(p.brand.opacity(if dark { 0.30 } else { 0.20 }))
    })
}

/// Media & content secondary pill button (36px).
pub(in crate::app) fn capsule_button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .secondary()
        .rounded(px(radius::FULL))
        .h(px(36.0))
        .px_4()
        .text_size(px(13.5))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
}

/// Compact media & content secondary pill button (32px) for cards and inline
/// preview headers.
pub(in crate::app) fn capsule_button_sm(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .secondary()
        .rounded(px(radius::FULL))
        .h(px(32.0))
        .px_3p5()
        .text_size(px(13.0))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
}

/// Circular secondary tonal icon button (e.g. Daily Mix queue append).
pub(in crate::app) fn capsule_icon_button(
    id: impl Into<ElementId>,
    icon: impl Into<Icon>,
    label: impl Into<SharedString>,
    size: f32,
) -> Button {
    let label = label.into();
    let button = Button::new(id)
        .icon(icon.into())
        .accessibility_label(label.clone())
        .tooltip(label)
        .secondary()
        .rounded(px(radius::FULL))
        .cursor_pointer();
    let button = if size < 36.0 { button.small() } else { button };
    button.w(px(size)).h(px(size))
}

/// Full-pill toggle button for filters, EQ presets, and segmented tabs.
pub(in crate::app) fn pill_filter_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    active: bool,
    cx: &App,
) -> Button {
    let p = palette(cx);
    let dark = cx.theme().is_dark();
    Button::new(id)
        .label(label)
        .custom(soft_toggle_style(active, cx))
        .selected(active)
        .rounded(px(radius::FULL))
        .h(px(32.0))
        .px_3p5()
        .text_size(px(13.0))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .when(active, |this| {
            this.border_1()
                .border_color(p.brand.opacity(if dark { 0.28 } else { 0.18 }))
        })
}

/// Utility / form primary action button (10px radius, medium weight).
pub(in crate::app) fn primary_button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .primary()
        .rounded(px(radius::BUTTON))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
}

/// Utility / form elevated surface button: crisp white surface with warm border
/// in light mode, elevated warm stone in dark mode. Uses `ButtonVariant::Default`
/// so theme `button*` tokens control normal, hover, active, and disabled states
/// cleanly without inheriting `input_background` page gray.
pub(in crate::app) fn surface_button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .rounded(px(radius::BUTTON))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
}

/// Utility / form quiet ghost text button (10px radius, medium weight).
pub(in crate::app) fn ghost_button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .ghost()
        .rounded(px(radius::BUTTON))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
}

/// Destructive primary action button (solid warm crimson).
pub(in crate::app) fn danger_button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .danger()
        .rounded(px(radius::BUTTON))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
}

/// Destructive secondary action button (tinted outline).
pub(in crate::app) fn danger_surface_button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .danger()
        .outline()
        .rounded(px(radius::BUTTON))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
}
