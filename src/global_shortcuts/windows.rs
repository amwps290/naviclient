//! Windows API adapter and native message polling on the registration thread.

use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};

use super::binding::Hotkey;
use super::registrations::{HotkeySystem, Registrations};
use super::{GlobalShortcutEvent, ShortcutAction, WorkerCommand};

struct NativeHotkeys;

impl HotkeySystem for NativeHotkeys {
    fn register(&mut self, id: i32, hotkey: Hotkey) -> bool {
        let mut modifiers = MOD_NOREPEAT;
        if hotkey.modifiers.control {
            modifiers |= MOD_CONTROL;
        }
        if hotkey.modifiers.alt {
            modifiers |= MOD_ALT;
        }
        if hotkey.modifiers.shift {
            modifiers |= MOD_SHIFT;
        }
        if hotkey.modifiers.platform {
            modifiers |= MOD_WIN;
        }
        unsafe { RegisterHotKey(std::ptr::null_mut(), id, modifiers, hotkey.virtual_key) != 0 }
    }

    fn unregister(&mut self, id: i32) {
        unsafe { UnregisterHotKey(std::ptr::null_mut(), id) };
    }
}

fn report(events: &Sender<GlobalShortcutEvent>, failures: Vec<ShortcutAction>) {
    let _ = events.send(GlobalShortcutEvent::RegistrationReport { failures });
}

pub(super) fn run(
    commands: Receiver<WorkerCommand>,
    events: Sender<GlobalShortcutEvent>,
    initial: Vec<(ShortcutAction, String)>,
) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_HOTKEY,
    };

    let mut registered = Registrations::new(NativeHotkeys);
    report(&events, registered.replace(initial));
    let mut running = true;
    while running {
        while let Ok(command) = commands.try_recv() {
            match command {
                WorkerCommand::Update(bindings) => report(&events, registered.replace(bindings)),
                WorkerCommand::Suspend => registered.clear(),
                WorkerCommand::Shutdown => {
                    running = false;
                    break;
                }
            }
        }

        unsafe {
            let mut message = std::mem::zeroed::<MSG>();
            while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                if message.message == WM_HOTKEY {
                    if let Some(action) = registered.action(message.wParam as i32) {
                        let _ = events.send(GlobalShortcutEvent::Triggered(action));
                    }
                } else {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
