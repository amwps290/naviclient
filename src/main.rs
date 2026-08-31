#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod app;
mod assets;
mod audio;
mod config;
mod models;
mod msg;
mod single_instance;
mod tray;

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};

use anyhow::Context as _;
use gpui::*;
use gpui_component::{Root, Theme, ThemeMode, TitleBar};

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

fn main() {
    match init_logging() {
        Ok(path) => log::info!("application starting; log_file={}", path.display()),
        Err(error) => eprintln!("failed to initialize file logging: {error:#}"),
    }

    // 单实例：若已有实例在运行，通知其激活窗口后退出。
    if single_instance::acquire() {
        log::info!("another instance is already running; activating it and exiting");
        return;
    }

    Application::new().with_assets(assets::Assets).run(|cx| {
        gpui_component::init(cx);
        let theme_preference = config::load().theme;

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1280.0), px(820.0)), cx)),
            titlebar: Some(TitleBar::title_bar_options()),
            window_decorations: Some(WindowDecorations::Client),
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
