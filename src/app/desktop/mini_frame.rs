//! Suppress desktop window frames while the mini card draws its own edges.

use crate::app::prelude::*;

#[cfg(target_os = "linux")]
mod linux;

pub(crate) fn init_ui(cx: &mut App) {
    #[cfg(target_os = "linux")]
    linux::register_before_kit(cx);
    gpui_kit::init(cx);
    #[cfg(target_os = "linux")]
    linux::register_after_kit(cx);
}

#[derive(Default)]
pub(in crate::app) struct MiniWindowFrame {
    #[cfg(target_os = "windows")]
    restore: Option<u32>,
    #[cfg(target_os = "windows")]
    restore_origin: Option<windows_sys::Win32::Foundation::POINT>,
}

impl MiniWindowFrame {
    pub(in crate::app) fn set_mini(&mut self, window: &mut Window, mini: bool, cx: &mut App) {
        #[cfg(target_os = "linux")]
        linux::set_mini(window, mini, cx);
        #[cfg(target_os = "windows")]
        if let Some(hwnd) = main_window_hwnd(window) {
            let hwnd = hwnd as windows_sys::Win32::Foundation::HWND;
            if let Err(error) = self.set_native_mini(hwnd, mini, window.is_maximized()) {
                log::warn!("unable to update mini player window frame: {error}");
            }
        }
        #[cfg(not(target_os = "linux"))]
        let _ = (window, mini, cx);
    }

    /// Put a restored window back where it was before mini mode. This is kept
    /// separate from frame restoration because GPUI resizes the main content
    /// asynchronously; its resize uses `SWP_NOMOVE`, so either completion order
    /// preserves this origin.
    pub(in crate::app) fn restore_position(&mut self, window: &Window) {
        #[cfg(target_os = "windows")]
        if let Some(hwnd) = main_window_hwnd(window) {
            if let Err(error) =
                self.restore_native_position(hwnd as windows_sys::Win32::Foundation::HWND)
            {
                log::warn!("unable to restore main window position: {error}");
            }
        }
        #[cfg(not(target_os = "windows"))]
        let _ = window;
    }
}

#[cfg(target_os = "windows")]
use windows_sys::Win32::{
    Foundation::{GetLastError, SetLastError, HWND, POINT, RECT},
    Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        MONITOR_DEFAULTTONULL,
    },
    UI::WindowsAndMessaging::{
        GetWindowLongPtrW, GetWindowRect, SendMessageW, SetWindowLongPtrW, SetWindowPos, GWL_STYLE,
        SPI_SETNONCLIENTMETRICS, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, WM_SETTINGCHANGE, WS_CAPTION, WS_MAXIMIZEBOX, WS_POPUP, WS_THICKFRAME,
    },
};

#[cfg(target_os = "windows")]
const FRAME_STYLES: u32 = WS_CAPTION | WS_THICKFRAME | WS_MAXIMIZEBOX | WS_POPUP;

#[cfg(target_os = "windows")]
impl MiniWindowFrame {
    fn set_native_mini(&mut self, hwnd: HWND, mini: bool, maximized: bool) -> std::io::Result<()> {
        if mini {
            if self.restore.is_some() {
                return Ok(());
            }
            let style = window_style(hwnd)?;
            self.restore_origin = (!maximized).then(|| window_origin(hwnd)).transpose()?;
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

    fn restore_native_position(&mut self, hwnd: HWND) -> std::io::Result<()> {
        let Some(saved) = self.restore_origin.take() else {
            return Ok(());
        };
        let origin = available_origin(saved)?;
        unsafe {
            if SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                origin.x,
                origin.y,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER,
            ) == 0
            {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn window_origin(hwnd: HWND) -> std::io::Result<POINT> {
    let mut rect = RECT::default();
    unsafe {
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(POINT {
        x: rect.left,
        y: rect.top,
    })
}

#[cfg(target_os = "windows")]
fn available_origin(saved: POINT) -> std::io::Result<POINT> {
    const REACHABLE_WIDTH: i32 = 64;
    const REACHABLE_HEIGHT: i32 = 32;
    let title_corner = RECT {
        left: saved.x,
        top: saved.y,
        right: saved.x.saturating_add(REACHABLE_WIDTH),
        bottom: saved.y.saturating_add(REACHABLE_HEIGHT),
    };
    unsafe {
        if !MonitorFromRect(&title_corner, MONITOR_DEFAULTTONULL).is_null() {
            return Ok(saved);
        }
        let monitor = MonitorFromRect(&title_corner, MONITOR_DEFAULTTONEAREST);
        if monitor.is_null() {
            return Ok(saved);
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return Err(std::io::Error::last_os_error());
        }
        // Only the title-bar corner must be reachable. The main resize follows
        // independently and the user can then drag or maximize the window.
        Ok(clamp_origin(saved, info.rcWork))
    }
}

#[cfg(target_os = "windows")]
fn clamp_origin(origin: POINT, work: RECT) -> POINT {
    const REACHABLE_WIDTH: i32 = 64;
    const REACHABLE_HEIGHT: i32 = 32;
    POINT {
        x: origin
            .x
            .clamp(work.left, work.right.saturating_sub(REACHABLE_WIDTH)),
        y: origin
            .y
            .clamp(work.top, work.bottom.saturating_sub(REACHABLE_HEIGHT)),
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
