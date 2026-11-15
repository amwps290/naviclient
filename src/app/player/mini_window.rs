//! Applying mini window transitions through GPUI and native window adapters.

use crate::app::prelude::*;

impl NavidromeApp {
    fn enter_mini_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.mini_window.enter(
            window.viewport_size(),
            window.is_maximized(),
            window_paddings(window),
        ) else {
            return;
        };
        self.state.settings.close();
        set_mini_window_surface(window, true, cx);
        self.mini_window.frame.set_mini(window, true);
        window.resize(target);
        set_always_on_top(window, true);
        cx.notify();
    }

    pub(in crate::app) fn exit_mini_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(restore) = self.mini_window.exit() else {
            return;
        };
        self.mini_window.surface.borrow_mut().release(window);
        set_mini_window_surface(window, false, cx);
        self.mini_window.frame.set_mini(window, false);
        window.resize(restore.size);
        if restore.maximized && !window.is_maximized() {
            window.zoom_window();
        }
        set_always_on_top(window, false);
        cx.notify();
    }

    pub(in crate::app) fn toggle_mini_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mini_window.is_mini() {
            self.exit_mini_mode(window, cx);
        } else {
            self.enter_mini_mode(window, cx);
        }
    }

    pub(in crate::app) fn sync_mini_window_size(&self, window: &mut Window) {
        if let Some(target) = self.mini_window.resize_target(window.viewport_size()) {
            window.resize(target);
        }
    }
}

fn set_mini_window_surface(window: &mut Window, mini: bool, cx: &mut App) {
    // The kit's Root paints a rectangular theme background behind our rounded
    // card. Both that surface and the native window must support transparency.
    gpui_kit::base::Root::update(window, cx, |root, _, cx| {
        root.style().background = mini.then(|| gpui_kit::transparent_black().into());
        cx.notify();
    });
    window.set_background_appearance(if mini {
        gpui_kit::WindowBackgroundAppearance::Transparent
    } else {
        // Removing the Root override also restores the current theme color.
        gpui_kit::WindowBackgroundAppearance::Opaque
    });
}

#[cfg(test)]
mod tests;
