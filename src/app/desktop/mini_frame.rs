//! Remove the Windows non-client frame while the mini card draws its own edges.

use crate::app::prelude::*;

#[derive(Default)]
pub(in crate::app) struct MiniWindowFrame {
    #[cfg(target_os = "windows")]
    restore: Option<u32>,
}

impl MiniWindowFrame {
    pub(in crate::app) fn set_mini(&mut self, window: &Window, mini: bool) {
        #[cfg(target_os = "windows")]
        if let Some(hwnd) = main_window_hwnd(window) {
            let hwnd = hwnd as windows_sys::Win32::Foundation::HWND;
            if let Err(error) = self.set_native_mini(hwnd, mini) {
                log::warn!("unable to update mini player window frame: {error}");
            }
        }
        #[cfg(not(target_os = "windows"))]
        let _ = (window, mini);
    }
}

#[cfg(target_os = "windows")]
use windows_sys::Win32::{
    Foundation::{GetLastError, SetLastError, HWND},
    UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SendMessageW, SetWindowLongPtrW, SetWindowPos, GWL_STYLE,
        SPI_SETNONCLIENTMETRICS, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, WM_SETTINGCHANGE, WS_CAPTION, WS_MAXIMIZEBOX, WS_POPUP, WS_THICKFRAME,
    },
};

#[cfg(target_os = "windows")]
const FRAME_STYLES: u32 = WS_CAPTION | WS_THICKFRAME | WS_MAXIMIZEBOX | WS_POPUP;

#[cfg(target_os = "windows")]
impl MiniWindowFrame {
    fn set_native_mini(&mut self, hwnd: HWND, mini: bool) -> std::io::Result<()> {
        if mini {
            if self.restore.is_some() {
                return Ok(());
            }
            let style = window_style(hwnd)?;
            self.restore = Some(style & FRAME_STYLES);
            apply_frame_style(hwnd, (style & !FRAME_STYLES) | WS_POPUP)?;
        } else if let Some(saved) = self.restore {
            // Preserve visibility and other flags changed while mini was open.
            let style = window_style(hwnd)?;
            apply_frame_style(hwnd, (style & !FRAME_STYLES) | saved)?;
            self.restore = None;
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn window_style(hwnd: HWND) -> std::io::Result<u32> {
    unsafe {
        SetLastError(0);
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
        if style == 0 && GetLastError() != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(style as u32)
    }
}

#[cfg(target_os = "windows")]
fn apply_frame_style(hwnd: HWND, style: u32) -> std::io::Result<()> {
    unsafe {
        SetLastError(0);
        if SetWindowLongPtrW(hwnd, GWL_STYLE, style as _) == 0 && GetLastError() != 0 {
            return Err(std::io::Error::last_os_error());
        }
        if SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        ) == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        // GPUI caches the difference between native and client bounds. Notify
        // only this window so it refreshes those metrics before the next resize.
        SendMessageW(hwnd, WM_SETTINGCHANGE, SPI_SETNONCLIENTMETRICS as usize, 0);
    }
    Ok(())
}

#[cfg(all(test, target_os = "windows"))]
mod tests;
