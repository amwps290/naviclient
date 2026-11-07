//! Shelves: the page is a stack of them, so new Home features add a shelf
//! rather than a rearrangement. Implemented presentations are large cards and
//! square covers; compact song rows and chip rows join when content needs them.

use crate::app::prelude::*;
use crate::app::style::grid::{GridColumns, GRID_GAP};

/// Square covers on the shared column grid.
pub(in crate::app) fn cover_grid(
    columns: GridColumns,
    items: impl IntoIterator<Item = gpui_kit::AnyElement>,
) -> gpui_kit::Div {
    div()
        .w_full()
        .grid()
        .grid_cols(columns.count as u16)
        .gap(px(GRID_GAP))
        .children(items)
}

/// Large cards sharing a row equally.
pub(in crate::app) fn hero_shelf(
    cards: impl IntoIterator<Item = gpui_kit::AnyElement>,
) -> gpui_kit::Div {
    h_flex()
        .w_full()
        .gap(px(GRID_GAP))
        .items_stretch()
        .children(
            cards
                .into_iter()
                .map(|card| div().flex_1().min_w_0().child(card)),
        )
}

pub(in crate::app) fn shelf_section(
    title: gpui_kit::AnyElement,
    content: impl IntoElement,
) -> gpui_kit::AnyElement {
    v_flex()
        .gap_3()
        .child(title)
        .child(content)
        .into_any_element()
}
