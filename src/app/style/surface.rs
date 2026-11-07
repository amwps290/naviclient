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
