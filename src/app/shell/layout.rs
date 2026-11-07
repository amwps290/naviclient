//! Window-level layout arithmetic. Computed from the current frame's window
//! size so grids never lag behind a resize or a drawer toggle.

use super::view_helpers::compact_window_layout;
use crate::app::prelude::*;
use crate::app::style::grid::{grid_columns, GridColumns};

#[derive(Clone, Copy, Debug)]
struct ShellInput {
    viewport_width: f32,
    window_padding: f32,
    sidebar_preference_collapsed: bool,
    queue_open: bool,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::app) struct ShellLayout {
    pub(in crate::app) compact: bool,
    pub(in crate::app) sidebar_collapsed: bool,
    pub(in crate::app) auto_collapsed: bool,
    pub(in crate::app) sidebar_width: f32,
    pub(in crate::app) queue_drawer_width: f32,
    pub(in crate::app) player_margin: f32,
    pub(in crate::app) page_padding: f32,
    pub(in crate::app) content_width: f32,
}

fn shell_layout(input: ShellInput) -> ShellLayout {
    let compact = compact_window_layout(input.viewport_width);
    let sidebar_collapsed = input.sidebar_preference_collapsed || compact;
    let sidebar_width = if sidebar_collapsed {
        tokens::SIDEBAR_COLLAPSED_WIDTH
    } else {
        tokens::SIDEBAR_WIDTH
    };
    let queue_drawer_width = if compact {
        tokens::QUEUE_DRAWER_COMPACT_WIDTH
    } else {
        tokens::QUEUE_DRAWER_WIDTH
    };
    let page_padding = if compact {
        tokens::PAGE_PADDING_COMPACT
    } else {
        tokens::PAGE_PADDING
    };
    let drawer = if input.queue_open {
        queue_drawer_width + tokens::QUEUE_DRAWER_MARGIN
    } else {
        0.0
    };
    let content_width =
        (input.viewport_width - input.window_padding - sidebar_width - drawer - page_padding * 2.0)
            .max(0.0);
    ShellLayout {
        compact,
        sidebar_collapsed,
        auto_collapsed: compact,
        sidebar_width,
        queue_drawer_width,
        player_margin: if compact {
            tokens::PLAYER_COMPACT_MARGIN
        } else {
            tokens::PLAYER_MARGIN
        },
        page_padding,
        content_width,
    }
}

impl NavidromeApp {
    pub(in crate::app) fn shell_layout(&self, window: &Window) -> ShellLayout {
        let paddings = window_paddings(window);
        shell_layout(ShellInput {
            viewport_width: f32::from(window.viewport_size().width),
            window_padding: f32::from(paddings.left + paddings.right),
            sidebar_preference_collapsed: self.config.sidebar_collapsed,
            queue_open: self.state.queue_drawer_open,
        })
    }

    pub(in crate::app) fn content_columns(&self, window: &Window) -> GridColumns {
        grid_columns(self.shell_layout(window).content_width)
    }
}

#[cfg(test)]
mod tests;
