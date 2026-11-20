use super::*;
use windows_sys::Win32::{
    Foundation::RECT,
    UI::{
        Input::KeyboardAndMouse::EnableWindow,
        WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, GetClientRect, GetWindowRect, SetWindowPos,
            SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER, WS_DISABLED, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
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

    fn origin(&self) -> (i32, i32) {
        let origin = window_origin(self.0).unwrap();
        (origin.x, origin.y)
    }

    fn move_to(&self, x: i32, y: i32) {
        unsafe {
            assert_ne!(
                SetWindowPos(
                    self.0,
                    std::ptr::null_mut(),
                    x,
                    y,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOSIZE | SWP_NOZORDER,
                ),
                0
            );
        }
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
        frame.set_native_mini(window.0, true, false).unwrap();
        let style = window_style(window.0).unwrap();
        assert_eq!(style & (WS_CAPTION | WS_THICKFRAME | WS_MAXIMIZEBOX), 0);
        assert_eq!(style & WS_POPUP, WS_POPUP);
        assert_eq!(style & WS_VISIBLE, 0);
        let (outer, client) = window.sizes();
        assert_eq!(outer, original_sizes.0);
        assert_eq!(outer, client, "mini must have no non-client border space");

        frame.set_native_mini(window.0, true, false).unwrap();
        frame.set_native_mini(window.0, false, false).unwrap();
        assert_eq!(window_style(window.0).unwrap(), original_style);
        assert_eq!(window.sizes(), original_sizes);
        frame.set_native_mini(window.0, false, false).unwrap();
        assert_eq!(window_style(window.0).unwrap(), original_style);
    }
}

#[test]
fn leaving_mini_restores_the_windowed_origin_once() {
    let window = HiddenWindow::new(WS_OVERLAPPEDWINDOW);
    let original = window.origin();
    let mut frame = MiniWindowFrame::default();

    frame.set_native_mini(window.0, true, false).unwrap();
    window.move_to(original.0 + 400, original.1 + 300);
    assert_ne!(window.origin(), original);
    frame.set_native_mini(window.0, false, false).unwrap();
    frame.restore_native_position(window.0).unwrap();
    assert_eq!(window.origin(), original);

    window.move_to(original.0 + 200, original.1 + 100);
    frame.restore_native_position(window.0).unwrap();
    assert_ne!(window.origin(), original, "restore position is single-use");
}

#[test]
fn unavailable_display_origins_keep_a_reachable_title_bar_corner() {
    let work = RECT {
        left: 100,
        top: 50,
        right: 2020,
        bottom: 1090,
    };
    let clamped = clamp_origin(POINT { x: -4000, y: 3000 }, work);
    assert_eq!((clamped.x, clamped.y), (100, 1058));
    let unchanged = clamp_origin(POINT { x: 500, y: 400 }, work);
    assert_eq!((unchanged.x, unchanged.y), (500, 400));
}

#[test]
fn leaving_mini_preserves_unrelated_window_flags_and_an_initially_borderless_window() {
    for initial_style in [WS_OVERLAPPEDWINDOW, WS_POPUP] {
        let window = HiddenWindow::new(initial_style);
        let original_style = window_style(window.0).unwrap();
        let mut frame = MiniWindowFrame::default();
        frame.set_native_mini(window.0, true, false).unwrap();
        unsafe {
            EnableWindow(window.0, 0);
        }
        frame.set_native_mini(window.0, false, false).unwrap();
        assert_eq!(
            window_style(window.0).unwrap(),
            original_style | WS_DISABLED
        );
    }
}
