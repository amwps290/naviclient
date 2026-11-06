//! Windows tray creation and independent native message loop.

use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};

use super::TrayCommand;

pub(super) fn start(tx: Sender<TrayCommand>) -> Result<()> {
    std::thread::Builder::new()
        .name("tray-worker".to_string())
        .spawn(move || {
            if let Err(error) = run_tray_loop(&tx) {
                log::warn!("tray worker failed: {error:#}");
            }
        })
        .map(|_| ())?;
    Ok(())
}

fn run_tray_loop(tx: &Sender<TrayCommand>) -> Result<()> {
    use rust_i18n::t;
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIconBuilder};

    let toggle = MenuItem::new(t!("Play / Pause"), true, None);
    let previous = MenuItem::new(t!("Previous"), true, None);
    let next = MenuItem::new(t!("Next"), true, None);
    let show = MenuItem::new(t!("Show window"), true, None);
    let quit = MenuItem::new(t!("Exit"), true, None);

    let menu = Menu::new();
    menu.append(&toggle)?;
    menu.append(&previous)?;
    menu.append(&next)?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&show)?;
    menu.append(&quit)?;

    let icon = Icon::from_rgba(tray_icon_rgba()?, 32, 32)
        .map_err(|error| anyhow!("invalid tray icon: {error}"))?;

    let _tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .with_tooltip("Navidrome Client")
        .build()
        .map_err(|error| anyhow!("failed to create tray icon: {error}"))?;
    log::info!("system tray icon created");

    // Windows 需要泵消息循环处理托盘交互；菜单事件从 muda 全局通道转发到应用。
    loop {
        pump_windows_messages();
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            let command = if event.id == toggle.id() {
                Some(TrayCommand::TogglePlayback)
            } else if event.id == previous.id() {
                Some(TrayCommand::Previous)
            } else if event.id == next.id() {
                Some(TrayCommand::Next)
            } else if event.id == show.id() {
                Some(TrayCommand::ShowWindow)
            } else if event.id == quit.id() {
                Some(TrayCommand::Quit)
            } else {
                None
            };
            if let Some(command) = command {
                let _ = tx.send(command);
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn pump_windows_messages() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };
    unsafe {
        let mut message = std::mem::zeroed::<MSG>();
        while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// 生成 32x32 RGBA 托盘图标（使用独立应用图标，而不是默认封面）。
fn tray_icon_rgba() -> Result<Vec<u8>> {
    let bytes = include_bytes!("../../assets/app-icon.png");
    let image = image::load_from_memory(bytes)
        .context("failed to decode tray icon")?
        .resize(32, 32, image::imageops::FilterType::Triangle)
        .to_rgba8();
    Ok(image.into_raw())
}
