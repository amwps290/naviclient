use super::*;
use windows_sys::Win32::{
    Foundation::RECT,
    UI::{
        Input::KeyboardAndMouse::EnableWindow,
        WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, GetClientRect, GetWindowRect, WS_DISABLED,
            WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        },
    },
};

struct HiddenWindow(HWND);

impl HiddenWindow {
    fn new(style: u32) -> Self {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                std::ptr::null(),
                style,
                50,
                50,
                320,
                160,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null(), "{}", std::io::Error::last_os_error());
        Self(hwnd)
    }

    fn sizes(&self) -> ((i32, i32), (i32, i32)) {
        let mut outer = RECT::default();
        let mut client = RECT::default();
        unsafe {
            assert_ne!(GetWindowRect(self.0, &mut outer), 0);
            assert_ne!(GetClientRect(self.0, &mut client), 0);
        }
        (
            (outer.right - outer.left, outer.bottom - outer.top),
            (client.right - client.left, client.bottom - client.top),
        )
    }
}

impl Drop for HiddenWindow {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.0);
        }
    }
}

#[test]
fn mini_removes_the_actual_native_frame_and_restores_it_on_exit() {
    let window = HiddenWindow::new(WS_OVERLAPPEDWINDOW);
    let original_style = window_style(window.0).unwrap();
    let original_sizes = window.sizes();
    assert_ne!(original_sizes.0, original_sizes.1);
    let mut frame = MiniWindowFrame::default();

    for _ in 0..3 {
        frame.set_native_mini(window.0, true).unwrap();
        let style = window_style(window.0).unwrap();
        assert_eq!(style & (WS_CAPTION | WS_THICKFRAME | WS_MAXIMIZEBOX), 0);
        assert_eq!(style & WS_POPUP, WS_POPUP);
        assert_eq!(style & WS_VISIBLE, 0);
        let (outer, client) = window.sizes();
        assert_eq!(outer, original_sizes.0);
        assert_eq!(outer, client, "mini must have no non-client border space");

        frame.set_native_mini(window.0, true).unwrap();
        frame.set_native_mini(window.0, false).unwrap();
        assert_eq!(window_style(window.0).unwrap(), original_style);
        assert_eq!(window.sizes(), original_sizes);
        frame.set_native_mini(window.0, false).unwrap();
        assert_eq!(window_style(window.0).unwrap(), original_style);
    }
}

#[test]
fn leaving_mini_preserves_unrelated_window_flags_and_an_initially_borderless_window() {
    for initial_style in [WS_OVERLAPPEDWINDOW, WS_POPUP] {
        let window = HiddenWindow::new(initial_style);
        let original_style = window_style(window.0).unwrap();
        let mut frame = MiniWindowFrame::default();
        frame.set_native_mini(window.0, true).unwrap();
        unsafe {
            EnableWindow(window.0, 0);
        }
        frame.set_native_mini(window.0, false).unwrap();
        assert_eq!(
            window_style(window.0).unwrap(),
            original_style | WS_DISABLED
        );
    }
}
