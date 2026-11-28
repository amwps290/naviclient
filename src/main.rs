#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// i18n：编译期嵌入 locales/*.yml；t! 缺键回退原文（键即英文）。
// 必须在 crate root 声明，子模块里 t! 才能解析 crate::_rust_i18n_try_translate。
rust_i18n::i18n!["locales", fallback = "en"];

mod api;
mod app;
mod assets;
mod audio;
mod bilibili_check;
mod bilibili_login_web;
mod config;
mod diagnostics;
mod global_shortcuts;
#[cfg(test)]
mod i18n_tests;
mod import;
mod local_playlists;
mod logging;
mod lyrics;
mod media_session;
mod memory_stats;
mod models;
mod msg;
mod notify;
mod offline;
mod playback_timeline;
mod playlist;
pub mod recommendation;
mod secrets;
mod single_instance;
pub mod source;
#[cfg(test)]
mod test_support;
mod tray;
mod typography;
#[cfg(test)]
mod ui_input_tests;
mod vinyl;

use gpui_kit::component::{Theme, ThemeMode, TitleBar};
use gpui_kit::*;

use crate::models::ThemePreference;

/// 渲染函数在 Debug 构建下会占用大量栈空间（`render_content` 单帧约 700 KB），
/// 默认 1 MB 的主线程栈会在首次渲染时溢出。Debug 保留 32 MB 栈，Release 下收紧到 8 MB。
#[cfg(debug_assertions)]
const APP_STACK_BYTES: usize = 32 * 1024 * 1024;
#[cfg(not(debug_assertions))]
const APP_STACK_BYTES: usize = 8 * 1024 * 1024;

fn main() {
    if bilibili_check::try_run() {
        return;
    }
    if bilibili_login_web::try_run() {
        return;
    }
    let file_logging = config::load().log_to_file;
    match logging::init(file_logging) {
        Ok(path) => log::info!(
            "application starting; file_logging={file_logging} log_file={}",
            path.display()
        ),
        Err(error) => eprintln!("failed to initialize logging: {error:#}"),
    }

    // 单实例：若已有实例在运行，通知其激活窗口后退出。
    if std::env::var("NAVI_TEST_MULTI").is_err() && single_instance::acquire() {
        log::info!("another instance is already running; activating it and exiting");
        return;
    }

    let app_thread = std::thread::Builder::new()
        .name("navidrome-ui".to_string())
        .stack_size(APP_STACK_BYTES)
        .spawn(run_app)
        .expect("failed to spawn UI thread");
    if let Err(error) = app_thread.join() {
        log::error!("UI thread panicked: {error:?}");
    }
}

fn run_app() {
    gpui_kit::application()
        .with_assets(assets::Assets)
        .run(|cx| {
            app::init_ui(cx);
            let config = config::load();
            let theme_preference = config.theme;
            let ui_locale = config.language.resolve();

            let window_options = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1280.0), px(820.0)), cx)),
                window_decorations: Some(WindowDecorations::Client),
                ..TitleBar::window_options()
            };

            cx.spawn(async move |cx| {
                cx.update(|cx| {
                    gpui_kit::open_window(window_options, cx, |window, cx| {
                        window.activate_window();
                        window.set_window_title("Navidrome Client");
                        match theme_preference {
                            ThemePreference::Light => {
                                Theme::change(ThemeMode::Light, Some(window), cx)
                            }
                            ThemePreference::Dark => {
                                Theme::change(ThemeMode::Dark, Some(window), cx)
                            }
                            ThemePreference::System => {
                                Theme::sync_system_appearance(Some(window), cx)
                            }
                        }
                        app::apply_app_theme(cx);
                        typography::configure_ui_font(cx, ui_locale);
                        typography::configure_serif_font(cx, ui_locale);

                        cx.new(|cx| app::NavidromeApp::new(window, cx))
                    })
                })?;

                Ok::<_, anyhow::Error>(())
            })
            .detach();
        });
}
