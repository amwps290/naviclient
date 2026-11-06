//! Shared tray commands and platform routing. Each backend owns its native resources.

use std::sync::mpsc::Sender;

use anyhow::Result;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(any(target_os = "linux", test))]
mod pixels;
#[cfg(target_os = "windows")]
mod windows;

/// 系统托盘菜单触发的播放器命令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub enum TrayCommand {
    TogglePlayback,
    Previous,
    Next,
    ShowWindow,
    Quit,
}

/// Start Windows' independent message loop or Linux's SNI service. Other
/// platforms remain a no-op; the app consumes commands through its 50ms poll.
pub fn start_tray_worker(tx: Sender<TrayCommand>) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        windows::start(tx)
    }
    #[cfg(target_os = "linux")]
    {
        linux::start(tx)
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = tx;
        log::warn!("system tray is not implemented on this platform");
        Ok(())
    }
}

/// Sync Linux's playback label and icon. Other platforms retain their existing
/// no-op behavior, as do Linux sessions without a tray host.
pub fn set_tray_paused(paused: bool) {
    #[cfg(target_os = "linux")]
    linux::set_paused(paused);
    #[cfg(not(target_os = "linux"))]
    let _ = paused;
}

/// Linux rebuilds its menu using the current locale; Windows' labels are fixed
/// when the tray starts and change on the next launch.
pub fn refresh_labels() {
    #[cfg(target_os = "linux")]
    linux::refresh_labels();
}
