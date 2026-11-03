use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// 可由用户重新绑定的应用内快捷键动作。枚举名作为配置文件中的稳定键。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutAction {
    TogglePlayback,
    Previous,
    Next,
    SeekBack,
    SeekForward,
    VolumeUp,
    VolumeDown,
    ToggleMute,
    ToggleNowPlaying,
    ToggleQueue,
    FocusSearch,
    CloseOverlays,
    SelectPrevious,
    SelectNext,
    SelectAll,
    PlaySelected,
}

impl ShortcutAction {
    pub const ALL: [Self; 16] = [
        Self::TogglePlayback,
        Self::Previous,
        Self::Next,
        Self::SeekBack,
        Self::SeekForward,
        Self::VolumeUp,
        Self::VolumeDown,
        Self::ToggleMute,
        Self::ToggleNowPlaying,
        Self::ToggleQueue,
        Self::FocusSearch,
        Self::CloseOverlays,
        Self::SelectPrevious,
        Self::SelectNext,
        Self::SelectAll,
        Self::PlaySelected,
    ];

    pub const GLOBAL_PLAYBACK: [Self; 8] = [
        Self::TogglePlayback,
        Self::Previous,
        Self::Next,
        Self::SeekBack,
        Self::SeekForward,
        Self::VolumeUp,
        Self::VolumeDown,
        Self::ToggleMute,
    ];

    pub const fn default_binding(self) -> &'static str {
        match self {
            Self::TogglePlayback => "space",
            Self::Previous => "ctrl-left",
            Self::Next => "ctrl-right",
            Self::SeekBack => "left",
            Self::SeekForward => "right",
            Self::VolumeUp => "up",
            Self::VolumeDown => "down",
            Self::ToggleMute => "m",
            Self::ToggleNowPlaying => "l",
            Self::ToggleQueue => "q",
            Self::FocusSearch => "ctrl-f",
            Self::CloseOverlays => "escape",
            Self::SelectPrevious => "shift-up",
            Self::SelectNext => "shift-down",
            Self::SelectAll => "ctrl-a",
            Self::PlaySelected => "enter",
        }
    }

    pub const fn default_global_binding(self) -> Option<&'static str> {
        match self {
            Self::TogglePlayback => Some("ctrl-alt-space"),
            Self::Previous => Some("ctrl-alt-left"),
            Self::Next => Some("ctrl-alt-right"),
            Self::SeekBack => Some("ctrl-alt-shift-left"),
            Self::SeekForward => Some("ctrl-alt-shift-right"),
            Self::VolumeUp => Some("ctrl-alt-up"),
            Self::VolumeDown => Some("ctrl-alt-down"),
            Self::ToggleMute => Some("ctrl-alt-m"),
            Self::ToggleNowPlaying
            | Self::ToggleQueue
            | Self::FocusSearch
            | Self::CloseOverlays
            | Self::SelectPrevious
            | Self::SelectNext
            | Self::SelectAll
            | Self::PlaySelected => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShortcutSettings {
    #[serde(default)]
    pub bindings: BTreeMap<ShortcutAction, String>,
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        Self {
            bindings: ShortcutAction::ALL
                .into_iter()
                .map(|action| (action, action.default_binding().to_string()))
                .collect(),
        }
    }
}

impl ShortcutSettings {
    pub fn binding(&self, action: ShortcutAction) -> &str {
        self.bindings
            .get(&action)
            .map(String::as_str)
            .unwrap_or_else(|| action.default_binding())
    }

    pub fn set(&mut self, action: ShortcutAction, binding: String) {
        self.bindings.insert(action, binding);
    }

    pub fn reset(&mut self, action: ShortcutAction) {
        self.bindings
            .insert(action, action.default_binding().to_string());
    }

    pub fn reset_all(&mut self) {
        *self = Self::default();
    }
}

/// 独立于窗口内快捷键的系统全局播放快捷键。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalShortcutSettings {
    #[serde(default)]
    pub bindings: BTreeMap<ShortcutAction, String>,
}

impl Default for GlobalShortcutSettings {
    fn default() -> Self {
        Self {
            bindings: ShortcutAction::GLOBAL_PLAYBACK
                .into_iter()
                .map(|action| {
                    (
                        action,
                        action
                            .default_global_binding()
                            .expect("global playback action must have a default")
                            .to_string(),
                    )
                })
                .collect(),
        }
    }
}

impl GlobalShortcutSettings {
    pub fn binding(&self, action: ShortcutAction) -> &str {
        self.bindings
            .get(&action)
            .map(String::as_str)
            .or_else(|| action.default_global_binding())
            .expect("only global playback actions have system-wide bindings")
    }

    pub fn set(&mut self, action: ShortcutAction, binding: String) {
        debug_assert!(action.default_global_binding().is_some());
        self.bindings.insert(action, binding);
    }

    pub fn reset(&mut self, action: ShortcutAction) {
        let binding = action
            .default_global_binding()
            .expect("only global playback actions can be reset");
        self.bindings.insert(action, binding.to_string());
    }

    pub fn reset_all(&mut self) {
        *self = Self::default();
    }
}
