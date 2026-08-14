use std::sync::mpsc::Sender;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};

/// 系统托盘菜单触发的播放器命令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    TogglePlayback,
    Previous,
    Next,
    ShowWindow,
    Quit,
}

/// 启动托盘工作线程。Windows 上创建托盘图标与菜单并转发事件；其他平台暂为 no-op。
pub fn start_tray_worker(tx: Sender<TrayCommand>) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        use std::thread;
        thread::Builder::new()
            .name("tray-worker".to_string())
            .spawn(move || {
                if let Err(error) = run_tray_loop(&tx) {
                    log::warn!("tray worker failed: {error:#}");
                }
            })
            .map(|_| ())?;
        Ok(())
    }
    #[cfg(not(target_os = "windows"))]
    {
        log::warn!("system tray is not implemented on this platform");
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn run_tray_loop(tx: &Sender<TrayCommand>) -> Result<()> {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIconBuilder};

    let toggle = MenuItem::new("Play / Pause", true, None);
    let previous = MenuItem::new("Previous", true, None);
    let next = MenuItem::new("Next", true, None);
    let show = MenuItem::new("Show window", true, None);
    let quit = MenuItem::new("Exit", true, None);

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

#[cfg(target_os = "windows")]
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

/// 生成 32x32 RGBA 托盘图标（使用默认封面图）。
fn tray_icon_rgba() -> Result<Vec<u8>> {
    let bytes = include_bytes!("../assets/default-cover.png");
    let image = image::load_from_memory(bytes)
        .context("failed to decode tray icon")?
        .resize(32, 32, image::imageops::FilterType::Triangle)
        .to_rgba8();
    Ok(image.into_raw())
}
