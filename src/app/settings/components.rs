//! Shared layout primitives for the settings panel.
//!
//! Keeps every settings section visually cohesive: grouped cards with subtle
//! borders, hairline row dividers, consistent label/control alignment, and
//! quiet uppercase-like group headers.

use crate::app::prelude::*;
use gpui_kit::Div;

/// Subtle card container grouping related settings rows.
pub(in crate::app) fn settings_card(cx: &App) -> Div {
    let p = palette(cx);
    let dark = cx.theme().is_dark();
    v_flex()
        .w_full()
        .rounded(px(tokens::radius::COVER))
        .bg(if dark {
            p.page.opacity(0.55)
        } else {
            p.page.opacity(0.60)
        })
        .border_1()
        .border_color(if dark {
            hsla(0.0, 0.0, 1.0, 0.07)
        } else {
            p.border.opacity(0.75)
        })
        .overflow_hidden()
}

/// Hairline horizontal divider placed between rows inside a `settings_card`.
pub(in crate::app) fn settings_divider(cx: &App) -> Div {
    let p = palette(cx);
    let dark = cx.theme().is_dark();
    div().w_full().h(px(1.0)).bg(if dark {
        hsla(0.0, 0.0, 1.0, 0.06)
    } else {
        p.border.opacity(0.65)
    })
}

/// Quiet section-group header placed directly above a `settings_card`.
pub(in crate::app) fn settings_group_header(
    title: impl Into<SharedString>,
    trailing: Option<gpui_kit::AnyElement>,
    cx: &App,
) -> gpui_kit::AnyElement {
    let p = palette(cx);
    h_flex()
        .w_full()
        .px_1()
        .justify_between()
        .items_center()
        .gap_3()
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.text_muted)
                .child(title.into()),
        )
        .when_some(trailing, |row, trailing| row.child(trailing))
        .into_any_element()
}

/// A labelled settings section group: optional group header + rounded card whose
/// rows are separated by hairline dividers.
pub(in crate::app) fn settings_group(
    title: Option<SharedString>,
    trailing: Option<gpui_kit::AnyElement>,
    rows: Vec<gpui_kit::AnyElement>,
    cx: &App,
) -> gpui_kit::AnyElement {
    let mut card = settings_card(cx);
    for (index, row) in rows.into_iter().enumerate() {
        if index > 0 {
            card = card.child(settings_divider(cx));
        }
        card = card.child(row);
    }

    v_flex()
        .w_full()
        .gap_2()
        .when_some(title, |group, title| {
            group.child(settings_group_header(title, trailing, cx))
        })
        .child(card)
        .into_any_element()
}

/// Standard horizontal settings row: left label (+ optional description),
/// right control.
pub(in crate::app) fn settings_row(
    label: impl Into<SharedString>,
    description: Option<SharedString>,
    control: gpui_kit::AnyElement,
    cx: &App,
) -> gpui_kit::AnyElement {
    let p = palette(cx);
    h_flex()
        .w_full()
        .px_4()
        .py_3p5()
        .justify_between()
        .items_center()
        .gap_4()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.text)
                        .child(label.into()),
                )
                .when_some(description, |col, description| {
                    col.child(
                        div()
                            .text_xs()
                            .text_color(p.text_secondary)
                            .child(description),
                    )
                }),
        )
        .child(div().flex_none().child(control))
        .into_any_element()
}
