//! Shared global-binding validation and a single parsed registration key.

use gpui_kit::{Keystroke, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationError {
    UnsafeModifiers,
    UnsupportedKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(any(target_os = "windows", test)), allow(dead_code))]
pub(super) struct Hotkey {
    pub(super) modifiers: Modifiers,
    pub(super) virtual_key: u32,
}

impl Hotkey {
    pub(super) fn parse(binding: &str) -> Result<Self, ValidationError> {
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
        let virtual_key = virtual_key(&keystroke.key).ok_or(ValidationError::UnsupportedKey)?;
        Ok(Self {
            modifiers,
            virtual_key,
        })
    }
}

pub fn validate(binding: &str) -> Result<(), ValidationError> {
    Hotkey::parse(binding).map(|_| ())
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
mod tests;
