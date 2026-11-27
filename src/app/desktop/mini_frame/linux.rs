//! Skip the kit's painted client frame in mini mode without changing native insets.

use std::cell::Cell;

use gpui_kit::base::{Root, RootPlugin};
use gpui_kit::{AnyElement, Decorations, Edges, Empty, Pixels};

use crate::app::prelude::*;

// gpui-component 0.7.1 adds this border outside the root surface, on top of
// window_paddings(). Reserve the same space even when not painting the frame.
const CLIENT_BORDER: Pixels = px(1.0);

struct CaptureMiniSurface;

#[derive(Default)]
struct MiniFrame {
    mini: bool,
    // The two decorate hooks move this element within the same Root render.
    unframed: Cell<Option<AnyElement>>,
}

pub(super) fn register_before_kit(cx: &mut App) {
    Root::register_plugin(cx, |_, _| CaptureMiniSurface);
}

pub(super) fn register_after_kit(cx: &mut App) {
    Root::register_plugin(cx, |_, _| MiniFrame::default());
}

pub(super) fn set_mini(window: &mut Window, mini: bool, cx: &mut App) {
    let frame = Root::read(window, cx)
        .plugin::<MiniFrame>()
        .expect("initialize the mini window frame before opening windows");
    frame.update(cx, |frame, cx| {
        frame.mini = mini;
        cx.notify();
    });
}

impl Render for CaptureMiniSurface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl RootPlugin for CaptureMiniSurface {
    fn decorate(
        &self,
        surface: AnyElement,
        root: &Root,
        window: &mut Window,
        cx: &mut App,
    ) -> impl IntoElement {
        let frame = root
            .plugin::<MiniFrame>()
            .expect("register both mini frame hooks before opening windows");
        let frame = frame.read(cx);
        if !frame.mini {
            return surface;
        }

        // Keep the complete Root, including its overlays and key context,
        // but don't send it through the kit's rectangular WindowBorder.
        let surface = match window.window_decorations() {
            Decorations::Client { tiling } => {
                pad_surface(surface, content_insets(window_paddings(window), tiling))
            }
            Decorations::Server => surface,
        };
        frame.unframed.set(Some(surface));
        Empty.into_any_element()
    }
}

impl Render for MiniFrame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl RootPlugin for MiniFrame {
    fn decorate(
        &self,
        surface: AnyElement,
        _: &Root,
        _: &mut Window,
        _: &mut App,
    ) -> impl IntoElement {
        // Discard the kit's wrapper before layout/paint: merely clipping it
        // leaves the shadow pixels inside that rectangle visible at the corners.
        self.unframed.take().unwrap_or(surface)
    }
}

fn content_insets(mut paddings: Edges<Pixels>, tiling: gpui_kit::Tiling) -> Edges<Pixels> {
    if !tiling.top {
        paddings.top += CLIENT_BORDER;
    }
    if !tiling.bottom {
        paddings.bottom += CLIENT_BORDER;
    }
    if !tiling.left {
        paddings.left += CLIENT_BORDER;
    }
    if !tiling.right {
        paddings.right += CLIENT_BORDER;
    }
    paddings
}

fn pad_surface(surface: AnyElement, insets: Edges<Pixels>) -> AnyElement {
    // Keep native client insets untouched to avoid resize feedback on Linux.
    div()
        .size_full()
        .pt(insets.top)
        .pb(insets.bottom)
        .pl(insets.left)
        .pr(insets.right)
        .child(div().size_full().overflow_hidden().child(surface))
        .into_any_element()
}

#[cfg(test)]
mod tests;
