//! Raised surfaces and cover shadows. Shadows only go on things that float.

use gpui_kit::component::dialog::Dialog;
use gpui_kit::component::{box_shadow, ActiveTheme};
use gpui_kit::{hsla, px, App, BoxShadow, Hsla, Styled};

use super::tokens::palette;

fn shadow_color(cx: &App, opacity: f32) -> Hsla {
    palette(cx).text.opacity(opacity)
}

/// Dark mode has no visible shadow, so raised things also get a lighter
/// surface and a 1px border.
pub(in crate::app) fn raised_surface<E: Styled>(element: E, radius: f32, cx: &App) -> E {
    let p = palette(cx);
    let element = element.bg(p.raised).rounded(px(radius));
    if cx.theme().is_dark() {
        element
            .border_1()
            .border_color(p.border)
            .shadow(vec![box_shadow(
                px(0.0),
                px(8.0),
                px(24.0),
                px(0.0),
                hsla(0.0, 0.0, 0.0, 0.35),
            )])
    } else {
        element.shadow(vec![
            box_shadow(px(0.0), px(1.0), px(2.0), px(0.0), shadow_color(cx, 0.06)),
            box_shadow(px(0.0), px(8.0), px(24.0), px(0.0), shadow_color(cx, 0.10)),
        ])
    }
}

/// Translucent floating glass surface for the player dock, popovers and overlays.
/// Optionally blends a whisper of the active cover color while preserving text contrast.
pub(in crate::app) fn translucent_surface<E: Styled>(
    element: E,
    radius: f32,
    tint: Option<Hsla>,
    cx: &App,
) -> E {
    let p = palette(cx);
    let dark = cx.theme().is_dark();
    let base = match tint {
        Some(accent) => p.raised.blend(
            hsla(
                accent.h,
                accent.s.min(0.45),
                if dark { 0.40 } else { 0.62 },
                1.0,
            )
            .opacity(if dark { 0.11 } else { 0.07 }),
        ),
        None => p.raised,
    };
    let element = element
        .bg(base.opacity(if dark { 0.82 } else { 0.86 }))
        .rounded(px(radius))
        .border_1();
    if dark {
        element
            .border_color(hsla(0.0, 0.0, 1.0, 0.10))
            .shadow(vec![box_shadow(
                px(0.0),
                px(10.0),
                px(28.0),
                px(-2.0),
                hsla(0.0, 0.0, 0.0, 0.45),
            )])
    } else {
        element.border_color(p.border.opacity(0.85)).shadow(vec![
            box_shadow(px(0.0), px(1.0), px(2.0), px(0.0), shadow_color(cx, 0.06)),
            box_shadow(
                px(0.0),
                px(10.0),
                px(28.0),
                px(-2.0),
                shadow_color(cx, 0.12),
            ),
        ])
    }
}

pub(in crate::app) fn cover_shadow(hovered: bool, cx: &App) -> Vec<BoxShadow> {
    let color = if cx.theme().is_dark() {
        hsla(0.0, 0.0, 0.0, if hovered { 0.6 } else { 0.4 })
    } else {
        shadow_color(cx, if hovered { 0.24 } else { 0.14 })
    };
    let (y, blur) = if hovered { (10.0, 22.0) } else { (6.0, 16.0) };
    vec![box_shadow(px(0.0), px(y), px(blur), px(0.0), color)]
}

/// Dialogs are raised surfaces; the kit's large radius token already gives 20px.
pub(in crate::app) fn raised_dialog(dialog: Dialog, cx: &App) -> Dialog {
    let p = palette(cx);
    let dialog = dialog.bg(p.raised);
    if cx.theme().is_dark() {
        dialog.border_color(p.border)
    } else {
        dialog.border_color(p.raised)
    }
}
