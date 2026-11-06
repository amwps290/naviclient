use super::Hotkey;
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

#[test]
fn parsed_keys_preserve_every_safe_modifier_combination_and_reject_function_modifiers() {
    for mask in 0..16 {
        for function in [false, true] {
            let mut parts = Vec::new();
            for (bit, name) in [(1, "ctrl"), (2, "alt"), (4, "shift"), (8, "win")] {
                if mask & bit != 0 {
                    parts.push(name);
                }
            }
            if function {
                parts.push("fn");
            }
            parts.push("m");
            let binding = parts.join("-");
            let safe = !function && (mask & 8 != 0 || parts.len() >= 3);
            let parsed = Hotkey::parse(&binding);
            assert_eq!(
                validate(&binding),
                parsed.as_ref().map(|_| ()).map_err(|error| *error)
            );
            if safe {
                let hotkey = parsed.unwrap();
                assert_eq!(hotkey.virtual_key, b'M' as u32);
                assert_eq!(hotkey.modifiers.control, mask & 1 != 0);
                assert_eq!(hotkey.modifiers.alt, mask & 2 != 0);
                assert_eq!(hotkey.modifiers.shift, mask & 4 != 0);
                assert_eq!(hotkey.modifiers.platform, mask & 8 != 0);
                assert!(!hotkey.modifiers.function);
            } else {
                assert_eq!(parsed, Err(ValidationError::UnsafeModifiers));
            }
        }
    }
}

#[test]
fn parsed_registration_keys_preserve_recorded_case_aliases_punctuation_and_function_boundaries() {
    let upper = Hotkey::parse("ctrl-alt-M").unwrap();
    assert_eq!(upper.virtual_key, b'M' as u32);
    assert!(upper.modifiers.shift);
    for alias in ["win", "super", "cmd"] {
        let hotkey = Hotkey::parse(&format!("{alias}-ENTER")).unwrap();
        assert_eq!(hotkey.virtual_key, 0x0d);
        assert!(hotkey.modifiers.platform);
        assert!(!hotkey.modifiers.shift);
    }
    for (binding, code) in [
        ("ctrl-alt--", 0xbd),
        ("ctrl-alt-;", 0xba),
        ("ctrl-alt-f1", 0x70),
        ("ctrl-alt-f24", 0x87),
    ] {
        assert_eq!(Hotkey::parse(binding).unwrap().virtual_key, code);
    }
    for binding in ["ctrl-alt-f0", "ctrl-alt-f25", "ctrl-alt-🙂"] {
        assert_eq!(Hotkey::parse(binding), Err(ValidationError::UnsupportedKey));
    }
}

#[test]
fn parsing_and_modifier_errors_keep_their_existing_precedence() {
    for (binding, error) in [
        ("ctrl-🙂", ValidationError::UnsafeModifiers),
        ("ctrl-alt-🙂", ValidationError::UnsupportedKey),
        ("ctrl-alt-fn-space", ValidationError::UnsafeModifiers),
        ("ctrl-alt-space-extra", ValidationError::UnsupportedKey),
    ] {
        assert_eq!(Hotkey::parse(binding), Err(error));
        assert_eq!(validate(binding), Err(error));
    }
}
