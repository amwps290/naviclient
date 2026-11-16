//! Hide the kit's rectangular client frame without changing native insets.

use gpui_kit::base::{Root, RootPlugin};
use gpui_kit::{AnyElement, Decorations, Edges, Pixels, Size};

use crate::app::prelude::*;

// gpui-component 0.7.1 draws this border outside the root surface, in addition
// to window_paddings(). Keep its layout and native inset stable across mini
// transitions: changing either can make Wayland/X11 resize the window twice.
const CLIENT_BORDER: Pixels = px(1.0);

#[derive(Default)]
struct MiniFrame {
    mini: bool,
}

pub(super) fn init(cx: &mut App) {
    // Register after the kit so this decoration surrounds its WindowBorder.
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

impl Render for MiniFrame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

impl RootPlugin for MiniFrame {
    fn decorate(
        &self,
        surface: AnyElement,
        _: &Root,
        window: &mut Window,
        _: &mut App,
    ) -> impl IntoElement {
        let Decorations::Client { tiling } = window.window_decorations() else {
            return surface;
        };
        if !self.mini {
            return surface;
        }
        let mut insets = window_paddings(window);
        if !tiling.top {
            insets.top += CLIENT_BORDER;
        }
        if !tiling.bottom {
            insets.bottom += CLIENT_BORDER;
        }
        if !tiling.left {
            insets.left += CLIENT_BORDER;
        }
        if !tiling.right {
            insets.right += CLIENT_BORDER;
        }
        clip_client_frame(surface, insets, window.viewport_size())
    }
}

fn clip_client_frame(
    surface: AnyElement,
    insets: Edges<Pixels>,
    viewport: Size<Pixels>,
) -> AnyElement {
    // Clip both paint and hitboxes to the card's content area. The decorated
    // surface retains its original window coordinates, so controls and dragging
    // keep their positions while the square border and shadow stay transparent.
    div()
        .size_full()
        .pt(insets.top)
        .pb(insets.bottom)
        .pl(insets.left)
        .pr(insets.right)
        .child(
            div().relative().size_full().overflow_hidden().child(
                div()
                    .absolute()
                    .left(-insets.left)
                    .top(-insets.top)
                    .w(viewport.width)
                    .h(viewport.height)
                    .child(surface),
            ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests;
