//! Mini window mode, fixed content sizing, and single-use main window restoration.

use gpui_kit::{px, size, Edges, Pixels, Size};

// WindowBorder adds client-side shadow padding on Linux. Adding those edges
// keeps the mini player's drawable content at 260x56 on every platform.
// This width leaves the title/artist column enough room to avoid constant scrolling.
const MINI_CONTENT_WIDTH: Pixels = px(260.0);
const MINI_CONTENT_HEIGHT: Pixels = px(56.0);
const RESIZE_TOLERANCE: Pixels = px(1.0);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MainWindowGeometry {
    pub(super) size: Size<Pixels>,
    pub(super) maximized: bool,
}

struct MiniWindow {
    restore: MainWindowGeometry,
    target: Size<Pixels>,
}

#[derive(Default)]
pub(super) struct MiniWindowState {
    mini: Option<MiniWindow>,
}

impl MiniWindowState {
    pub(super) fn is_mini(&self) -> bool {
        self.mini.is_some()
    }

    /// A repeated entry must not overwrite the main window with mini geometry.
    pub(super) fn enter(
        &mut self,
        viewport: Size<Pixels>,
        maximized: bool,
        paddings: Edges<Pixels>,
    ) -> Option<Size<Pixels>> {
        if self.is_mini() {
            return None;
        }
        let target = size(
            MINI_CONTENT_WIDTH + paddings.left + paddings.right,
            MINI_CONTENT_HEIGHT + paddings.top + paddings.bottom,
        );
        self.mini = Some(MiniWindow {
            restore: MainWindowGeometry {
                size: viewport,
                maximized,
            },
            target,
        });
        Some(target)
    }

    /// Stale mini-view clicks after an exit cannot resize the main window again.
    pub(super) fn exit(&mut self) -> Option<MainWindowGeometry> {
        self.mini.take().map(|mini| mini.restore)
    }

    pub(super) fn resize_target(&self, viewport: Size<Pixels>) -> Option<Size<Pixels>> {
        let target = self.mini.as_ref()?.target;
        let resized = (viewport.width - target.width).abs() > RESIZE_TOLERANCE
            || (viewport.height - target.height).abs() > RESIZE_TOLERANCE;
        resized.then_some(target)
    }
}

#[cfg(test)]
mod tests;
