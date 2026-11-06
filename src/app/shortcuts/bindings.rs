//! Shortcut matching, labels, display formatting, and binding conflicts.

use gpui_kit::Keystroke;
use rust_i18n::t;

use crate::models::ShortcutAction;

/// 按用户配置匹配快捷键动作。配置缺少新增动作时由 `binding` 回退默认值。
pub(in crate::app) fn match_shortcut(
    settings: &crate::models::ShortcutSettings,
    keystroke: &Keystroke,
) -> Option<ShortcutAction> {
    ShortcutAction::ALL.into_iter().find(|action| {
        Keystroke::parse(settings.binding(*action)).is_ok_and(|binding| {
            binding.key == keystroke.key && binding.modifiers == keystroke.modifiers
        })
    })
}

pub(in crate::app) fn shortcut_label(action: ShortcutAction) -> std::borrow::Cow<'static, str> {
    match action {
        ShortcutAction::TogglePlayback => t!("Play / pause"),
        ShortcutAction::Previous => t!("Previous track"),
        ShortcutAction::Next => t!("Next track"),
        ShortcutAction::SeekBack => t!("Seek backward 5 seconds"),
        ShortcutAction::SeekForward => t!("Seek forward 5 seconds"),
        ShortcutAction::VolumeUp => t!("Volume up"),
        ShortcutAction::VolumeDown => t!("Volume down"),
        ShortcutAction::ToggleMute => t!("Mute / unmute"),
        ShortcutAction::ToggleNowPlaying => t!("Open / close now playing"),
        ShortcutAction::ToggleQueue => t!("Open / close queue"),
        ShortcutAction::FocusSearch => t!("Focus search"),
        ShortcutAction::CloseOverlays => t!("Close overlay / go back"),
        ShortcutAction::SelectPrevious => t!("Select previous song"),
        ShortcutAction::SelectNext => t!("Select next song"),
        ShortcutAction::SelectAll => t!("Select all songs"),
        ShortcutAction::PlaySelected => t!("Play selected song"),
    }
}

pub(in crate::app) fn shortcut_display(binding: &str) -> String {
    Keystroke::parse(binding)
        .map(|keystroke| keystroke.to_string())
        .unwrap_or_else(|_| binding.to_string())
}

pub(in crate::app) fn shortcut_conflict(
    settings: &crate::models::ShortcutSettings,
    target: ShortcutAction,
    binding: &str,
) -> Option<ShortcutAction> {
    let normalized = Keystroke::parse(binding).ok()?.unparse();
    ShortcutAction::ALL.into_iter().find(|action| {
        *action != target
            && Keystroke::parse(settings.binding(*action))
                .is_ok_and(|candidate| candidate.unparse() == normalized)
    })
}

pub(in crate::app) fn global_shortcut_conflict(
    settings: &crate::models::GlobalShortcutSettings,
    target: ShortcutAction,
    binding: &str,
) -> Option<ShortcutAction> {
    let normalized = Keystroke::parse(binding).ok()?.unparse();
    ShortcutAction::GLOBAL_PLAYBACK.into_iter().find(|action| {
        *action != target
            && Keystroke::parse(settings.binding(*action))
                .is_ok_and(|candidate| candidate.unparse() == normalized)
    })
}

#[cfg(test)]
mod tests;
