//! 原生窗口句柄、显示状态、关闭请求和置顶的平台适配。

use super::*;

#[cfg(target_os = "windows")]
pub(super) fn main_window_hwnd(window: &Window) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return None;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return None;
    };
    Some(handle.hwnd.get())
}

#[cfg(not(target_os = "windows"))]
pub(super) fn main_window_hwnd(_window: &Window) -> Option<isize> {
    None
}

#[cfg(target_os = "windows")]
pub(super) fn hide_main_window(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    unsafe {
        ShowWindow(hwnd as windows_sys::Win32::Foundation::HWND, SW_HIDE);
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn hide_main_window(_hwnd: isize) {}

#[cfg(target_os = "windows")]
pub(super) fn show_main_window(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_SHOW};
    unsafe {
        ShowWindow(hwnd as windows_sys::Win32::Foundation::HWND, SW_SHOW);
        SetForegroundWindow(hwnd as windows_sys::Win32::Foundation::HWND);
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn show_main_window(_hwnd: isize) {}

#[cfg(target_os = "windows")]
pub(super) fn request_window_close(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
    unsafe {
        PostMessageW(hwnd as windows_sys::Win32::Foundation::HWND, WM_CLOSE, 0, 0);
    }
}

#[cfg(target_os = "windows")]
pub(super) fn set_always_on_top(window: &mut Window, on: bool) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = window.window_handle() else {
        log::warn!("unable to obtain the native window handle");
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
    let insert_after = if on {
        windows_sys::Win32::UI::WindowsAndMessaging::HWND_TOPMOST
    } else {
        windows_sys::Win32::UI::WindowsAndMessaging::HWND_NOTOPMOST
    };
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SetWindowPos(
            hwnd,
            insert_after,
            0,
            0,
            0,
            0,
            windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
                | windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOSIZE,
        );
    }
}

#[cfg(not(target_os = "windows"))]
pub(super) fn set_always_on_top(_window: &mut Window, on: bool) {
    log::warn!(
        "always-on-top is not implemented on this platform; the mini player will not stay on top (requested={on})"
    );
}
