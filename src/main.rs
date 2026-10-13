#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod app;
mod assets;
mod audio;
mod config;
mod media_session;
mod models;
mod msg;
mod playlist;
mod single_instance;
mod tray;
mod vinyl;

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};

use anyhow::Context as _;
use gpui_kit::component::{Theme, ThemeMode, TitleBar};
use gpui_kit::*;

use crate::models::ThemePreference;

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

struct LogWriter {
    file: File,
    mirror_to_stderr: bool,
}

impl Write for LogWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.file.write_all(buffer)?;
        if self.mirror_to_stderr {
            let _ = io::stderr().write_all(buffer);
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()?;
        if self.mirror_to_stderr {
            let _ = io::stderr().flush();
        }
        Ok(())
    }
}

fn init_logging() -> anyhow::Result<std::path::PathBuf> {
    let path = config::log_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create log directory")?;
    }
    if path
        .metadata()
        .is_ok_and(|metadata| metadata.len() >= MAX_LOG_BYTES)
    {
        let old_path = path.with_extension("log.old");
        let _ = fs::remove_file(&old_path);
        fs::rename(&path, old_path).context("failed to rotate log file")?;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .context("failed to open log file")?;
    let writer = LogWriter {
        file,
        mirror_to_stderr: cfg!(debug_assertions),
    };
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,navidrome_client=debug"),
    )
    .format_timestamp_millis()
    .target(env_logger::Target::Pipe(Box::new(writer)))
    .try_init()
    .context("failed to initialize logger")?;
    Ok(path)
}

/// 渲染函数在 Debug 构建下会占用大量栈空间（`render_content` 单帧约 700 KB），
/// 默认 1 MB 的主线程栈会在首次渲染时溢出。把应用跑在带大栈的专用线程上。
const APP_STACK_BYTES: usize = 32 * 1024 * 1024;

fn main() {
    match init_logging() {
        Ok(path) => log::info!("application starting; log_file={}", path.display()),
        Err(error) => eprintln!("failed to initialize file logging: {error:#}"),
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
            gpui_kit::init(cx);
            let theme_preference = config::load().theme;

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

                        cx.new(|cx| app::NavidromeApp::new(window, cx))
                    })
                })?;

                Ok::<_, anyhow::Error>(())
            })
            .detach();
        });
}
