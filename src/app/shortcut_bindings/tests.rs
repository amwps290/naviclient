use super::*;
use crate::models::{GlobalShortcutSettings, ShortcutSettings};
use gpui_kit::Modifiers;

fn default_shortcut(key: &str, modifiers: Modifiers) -> Option<ShortcutAction> {
    match_shortcut(
        &ShortcutSettings::default(),
        &Keystroke {
            key: key.to_string(),
            modifiers,
            key_char: None,
        },
    )
}

#[test]
fn shortcut_mapping_covers_playback_controls() {
    let none = Modifiers::default();
    assert_eq!(
        default_shortcut("space", none),
        Some(ShortcutAction::TogglePlayback)
    );
    assert_eq!(
        default_shortcut("left", none),
        Some(ShortcutAction::SeekBack)
    );
    assert_eq!(
        default_shortcut("right", none),
        Some(ShortcutAction::SeekForward)
    );
    assert_eq!(default_shortcut("up", none), Some(ShortcutAction::VolumeUp));
    assert_eq!(
        default_shortcut("down", none),
        Some(ShortcutAction::VolumeDown)
    );
    assert_eq!(
        default_shortcut("m", none),
        Some(ShortcutAction::ToggleMute)
    );
    assert_eq!(
        default_shortcut("l", none),
        Some(ShortcutAction::ToggleNowPlaying)
    );
    assert_eq!(
        default_shortcut("q", none),
        Some(ShortcutAction::ToggleQueue)
    );
    assert_eq!(
        default_shortcut("escape", none),
        Some(ShortcutAction::CloseOverlays)
    );
    assert_eq!(default_shortcut("x", none), None);
}

#[test]
fn shortcut_mapping_uses_ctrl_for_navigation_and_search() {
    let ctrl = Modifiers {
        control: true,
        ..Modifiers::default()
    };
    assert_eq!(
        default_shortcut("f", ctrl),
        Some(ShortcutAction::FocusSearch)
    );
    assert_eq!(
        default_shortcut("left", ctrl),
        Some(ShortcutAction::Previous)
    );
    assert_eq!(default_shortcut("right", ctrl), Some(ShortcutAction::Next));
    assert_eq!(default_shortcut("space", ctrl), None);
}

#[test]
fn shortcut_mapping_selects_list_rows_with_shift_and_plays_with_enter() {
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    let none = Modifiers::default();
    assert_eq!(
        default_shortcut("up", shift),
        Some(ShortcutAction::SelectPrevious)
    );
    assert_eq!(
        default_shortcut("down", shift),
        Some(ShortcutAction::SelectNext)
    );
    // 裸方向键仍是音量控制，不被 Shift 导航占用。
    assert_eq!(default_shortcut("up", none), Some(ShortcutAction::VolumeUp));
    assert_eq!(
        default_shortcut("down", none),
        Some(ShortcutAction::VolumeDown)
    );
    assert_eq!(
        default_shortcut("enter", none),
        Some(ShortcutAction::PlaySelected)
    );
    assert_eq!(default_shortcut("enter", shift), None);
}

#[test]
fn shortcut_mapping_selects_all_with_ctrl_a_only() {
    let ctrl = Modifiers {
        control: true,
        ..Modifiers::default()
    };
    let none = Modifiers::default();
    assert_eq!(default_shortcut("a", ctrl), Some(ShortcutAction::SelectAll));
    // 无修饰或 Shift 组合不触发全选。
    assert_eq!(default_shortcut("a", none), None);
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    assert_eq!(default_shortcut("a", shift), None);
    // 既有 Ctrl 快捷键不受影响。
    assert_eq!(
        default_shortcut("f", ctrl),
        Some(ShortcutAction::FocusSearch)
    );
}

#[test]
fn custom_shortcut_replaces_default_and_reports_conflicts() {
    let mut settings = ShortcutSettings::default();
    settings.set(ShortcutAction::TogglePlayback, "ctrl-p".to_string());

    assert_eq!(
        match_shortcut(&settings, &Keystroke::parse("ctrl-p").unwrap()),
        Some(ShortcutAction::TogglePlayback)
    );
    assert_eq!(
        match_shortcut(&settings, &Keystroke::parse("space").unwrap()),
        None
    );
    assert_eq!(
        shortcut_conflict(&settings, ShortcutAction::Next, "ctrl-p"),
        Some(ShortcutAction::TogglePlayback)
    );
    assert_eq!(
        shortcut_conflict(&settings, ShortcutAction::Next, "ctrl-n"),
        None
    );

    settings.set(ShortcutAction::Next, "space".to_string());
    assert_eq!(
        shortcut_conflict(
            &settings,
            ShortcutAction::TogglePlayback,
            ShortcutAction::TogglePlayback.default_binding()
        ),
        Some(ShortcutAction::Next)
    );
}

#[test]
fn global_shortcut_conflicts_are_scoped_to_global_playback_bindings() {
    let mut settings = GlobalShortcutSettings::default();
    settings.set(ShortcutAction::Next, "ctrl-alt-n".to_string());

    assert_eq!(
        global_shortcut_conflict(&settings, ShortcutAction::Previous, "ctrl-alt-n"),
        Some(ShortcutAction::Next)
    );
    assert_eq!(
        global_shortcut_conflict(&settings, ShortcutAction::Previous, "ctrl-alt-p"),
        None
    );
}
