#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod app;
mod audio;
mod config;
mod models;
mod msg;

use gpui::*;
use gpui_component::{Root, Theme, ThemeMode};

use crate::models::ThemePreference;

fn main() {
    env_logger::init();

    Application::new().run(|cx| {
        gpui_component::init(cx);
        let theme_preference = config::load().theme;

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1280.0), px(820.0)), cx)),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                window.activate_window();
                window.set_window_title("Navidrome Client");
                match theme_preference {
                    ThemePreference::Light => Theme::change(ThemeMode::Light, Some(window), cx),
                    ThemePreference::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
                    ThemePreference::System => Theme::sync_system_appearance(Some(window), cx),
                }

                let view = cx.new(|cx| app::NavidromeApp::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })?;

            Ok::<_, anyhow::Error>(())
        })
        .detach();
    });
}
