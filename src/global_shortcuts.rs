//! 系统全局播放快捷键。Windows 使用 `RegisterHotKey` 在线程消息队列上
//! 注册；其他平台保留相同接口并由设置页明确提示暂不支持自定义全局键。

use std::sync::mpsc::{self, Receiver, Sender};

use gpui_kit::Keystroke;

use crate::models::{GlobalShortcutSettings, ShortcutAction};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationError {
    UnsafeModifiers,
    UnsupportedKey,
}

#[derive(Debug)]
pub enum GlobalShortcutEvent {
    Triggered(ShortcutAction),
    RegistrationReport { failures: Vec<ShortcutAction> },
}

#[cfg(target_os = "windows")]
enum WorkerCommand {
    Update(Vec<(ShortcutAction, String)>),
    Suspend,
    Shutdown,
}

pub struct GlobalShortcutManager {
    events: Receiver<GlobalShortcutEvent>,
    #[cfg(target_os = "windows")]
    commands: Sender<WorkerCommand>,
    #[cfg(target_os = "windows")]
    worker: Option<std::thread::JoinHandle<()>>,
}

impl GlobalShortcutManager {
    pub fn new(settings: &GlobalShortcutSettings) -> Self {
        let (event_tx, events) = mpsc::channel();

        #[cfg(target_os = "windows")]
        {
            let (commands, command_rx) = mpsc::channel();
            let initial = snapshot(settings);
            let worker = std::thread::Builder::new()
                .name("global-shortcuts".to_string())
                .spawn(move || windows_worker(command_rx, event_tx, initial))
                .map_err(|error| {
                    log::warn!("failed to start global shortcut worker: {error}");
                    error
                })
                .ok();
            Self {
                events,
                commands,
                worker,
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            drop(event_tx);
            let _ = settings;
            Self { events }
        }
    }

    pub fn update(&self, settings: &GlobalShortcutSettings) {
        #[cfg(target_os = "windows")]
        {
            let _ = self
                .commands
                .send(WorkerCommand::Update(snapshot(settings)));
        }
        #[cfg(not(target_os = "windows"))]
        let _ = settings;
    }

    /// 录制组合键时解除旧注册，保证按键能送到当前窗口。
    pub fn suspend(&self) {
        #[cfg(target_os = "windows")]
        {
            let _ = self.commands.send(WorkerCommand::Suspend);
        }
    }

    pub fn try_recv(&self) -> Option<GlobalShortcutEvent> {
        self.events.try_recv().ok()
    }
}

#[cfg(target_os = "windows")]
impl Drop for GlobalShortcutManager {
    fn drop(&mut self) {
        let _ = self.commands.send(WorkerCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn validate(binding: &str) -> Result<(), ValidationError> {
    let keystroke = Keystroke::parse(binding).map_err(|_| ValidationError::UnsupportedKey)?;
    let modifiers = keystroke.modifiers;
    let modifier_count = [
        modifiers.control,
        modifiers.alt,
        modifiers.shift,
        modifiers.platform,
    ]
    .into_iter()
    .filter(|enabled| *enabled)
    .count();
    if modifiers.function || (!modifiers.platform && modifier_count < 2) {
        return Err(ValidationError::UnsafeModifiers);
    }
    virtual_key(&keystroke.key)
        .map(|_| ())
        .ok_or(ValidationError::UnsupportedKey)
}

#[cfg(target_os = "windows")]
fn snapshot(settings: &GlobalShortcutSettings) -> Vec<(ShortcutAction, String)> {
    ShortcutAction::GLOBAL_PLAYBACK
        .into_iter()
        .map(|action| (action, settings.binding(action).to_string()))
        .collect()
}

#[cfg(target_os = "windows")]
fn windows_worker(
    commands: Receiver<WorkerCommand>,
    events: Sender<GlobalShortcutEvent>,
    initial: Vec<(ShortcutAction, String)>,
) {
    use std::collections::HashMap;
    use std::time::Duration;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE, WM_HOTKEY,
    };

    const FIRST_ID: i32 = 0x4e00;
    let mut registered = HashMap::<i32, ShortcutAction>::new();

    let unregister_all = |registered: &mut HashMap<i32, ShortcutAction>| {
        for id in registered.keys().copied().collect::<Vec<_>>() {
            unsafe { UnregisterHotKey(std::ptr::null_mut(), id) };
        }
        registered.clear();
    };

    let apply = |bindings: Vec<(ShortcutAction, String)>,
                 registered: &mut HashMap<i32, ShortcutAction>| {
        unregister_all(registered);

        let mut failures = Vec::new();
        for (index, (action, binding)) in bindings.into_iter().enumerate() {
            if validate(&binding).is_err() {
                failures.push(action);
                continue;
            }
            let Ok(keystroke) = Keystroke::parse(&binding) else {
                failures.push(action);
                continue;
            };
            let Some(vk) = virtual_key(&keystroke.key) else {
                failures.push(action);
                continue;
            };
            let mut modifiers = MOD_NOREPEAT;
            if keystroke.modifiers.control {
                modifiers |= MOD_CONTROL;
            }
            if keystroke.modifiers.alt {
                modifiers |= MOD_ALT;
            }
            if keystroke.modifiers.shift {
                modifiers |= MOD_SHIFT;
            }
            if keystroke.modifiers.platform {
                modifiers |= MOD_WIN;
            }
            let id = FIRST_ID + index as i32;
            if unsafe { RegisterHotKey(std::ptr::null_mut(), id, modifiers, vk) } == 0 {
                failures.push(action);
            } else {
                registered.insert(id, action);
            }
        }
        let _ = events.send(GlobalShortcutEvent::RegistrationReport { failures });
    };

    apply(initial, &mut registered);
    let mut running = true;
    while running {
        while let Ok(command) = commands.try_recv() {
            match command {
                WorkerCommand::Update(bindings) => apply(bindings, &mut registered),
                WorkerCommand::Suspend => unregister_all(&mut registered),
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
                    if let Some(action) = registered.get(&(message.wParam as i32)).copied() {
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

    unregister_all(&mut registered);
}

/// Windows virtual-key code。只接受录制器会产生且适合作为快捷键的稳定键名。
fn virtual_key(key: &str) -> Option<u32> {
    if key.len() == 1 {
        let byte = key.as_bytes()[0];
        if byte.is_ascii_alphabetic() {
            return Some(byte.to_ascii_uppercase() as u32);
        }
        if byte.is_ascii_digit() {
            return Some(byte as u32);
        }
        return Some(match byte {
            b';' => 0xba,
            b'=' => 0xbb,
            b',' => 0xbc,
            b'-' => 0xbd,
            b'.' => 0xbe,
            b'/' => 0xbf,
            b'`' => 0xc0,
            b'[' => 0xdb,
            b'\\' => 0xdc,
            b']' => 0xdd,
            b'\'' => 0xde,
            _ => return None,
        });
    }

    match key {
        "space" => Some(0x20),
        "backspace" => Some(0x08),
        "enter" => Some(0x0d),
        "tab" => Some(0x09),
        "escape" => Some(0x1b),
        "left" => Some(0x25),
        "up" => Some(0x26),
        "right" => Some(0x27),
        "down" => Some(0x28),
        "home" => Some(0x24),
        "end" => Some(0x23),
        "pageup" => Some(0x21),
        "pagedown" => Some(0x22),
        "insert" => Some(0x2d),
        "delete" => Some(0x2e),
        key if key.starts_with('f') => key[1..]
            .parse::<u32>()
            .ok()
            .filter(|number| (1..=24).contains(number))
            .map(|number| 0x6f + number),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{validate, virtual_key, ValidationError};

    #[test]
    fn global_shortcuts_require_safe_modifiers() {
        assert_eq!(validate("space"), Err(ValidationError::UnsafeModifiers));
        assert_eq!(
            validate("ctrl-space"),
            Err(ValidationError::UnsafeModifiers)
        );
        assert_eq!(validate("ctrl-alt-space"), Ok(()));
        assert_eq!(validate("win-space"), Ok(()));
    }

    #[test]
    fn global_shortcuts_support_common_recorded_keys() {
        assert_eq!(virtual_key("left"), Some(0x25));
        assert_eq!(virtual_key("m"), Some(b'M' as u32));
        assert_eq!(virtual_key("f12"), Some(0x7b));
        assert_eq!(
            validate("ctrl-alt-🙂"),
            Err(ValidationError::UnsupportedKey)
        );
    }
}
