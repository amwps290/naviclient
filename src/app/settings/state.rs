//! Settings overlay state and mutually exclusive shortcut recording sessions.

use std::collections::HashSet;

use crate::global_shortcuts::ValidationError;
use crate::models::{Config, ServerProfile, ShortcutAction};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::app) enum SettingsSection {
    #[default]
    Servers,
    Playback,
    Desktop,
    Appearance,
    Shortcuts,
}

impl SettingsSection {
    pub(in crate::app) const ALL: [Self; 5] = [
        Self::Servers,
        Self::Playback,
        Self::Desktop,
        Self::Appearance,
        Self::Shortcuts,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum ShortcutRecording {
    Local(ShortcutAction),
    Global(ShortcutAction),
}

#[derive(Default)]
pub(in crate::app) struct SettingsUi {
    pub(in crate::app) bilibili_login: super::bilibili_state::LoginState,
    pub(in crate::app) open: bool,
    pub(in crate::app) section: SettingsSection,
    recording: Option<ShortcutRecording>,
    /// The action being edited and the action already using its binding.
    pub(in crate::app) shortcut_conflict: Option<(ShortcutAction, ShortcutAction)>,
    pub(in crate::app) global_shortcut_conflict: Option<(ShortcutAction, ShortcutAction)>,
    pub(in crate::app) global_shortcut_validation: Option<(ShortcutAction, ValidationError)>,
    /// Registration failures survive changes of the recording target.
    pub(in crate::app) global_shortcut_failures: HashSet<ShortcutAction>,
    /// Editing another profile does not switch the active server.
    /// Reconnecting resets the entire settings state and closes this form.
    pub(in crate::app) editing_server: Option<String>,
}

impl SettingsUi {
    pub(in crate::app) fn close(&mut self) {
        self.open = false;
        self.bilibili_login.cancel();
    }
    /// The visible form and Save/Cancel actions must address the same profile.
    /// An obsolete explicit edit target must never fall back to another server.
    pub(in crate::app) fn server_form_target<'a>(
        &self,
        config: &'a Config,
    ) -> Option<&'a ServerProfile> {
        match self.editing_server.as_deref() {
            Some(id) => config.servers.iter().find(|profile| profile.id == id),
            None => config
                .active_profile()
                .filter(|profile| !profile.is_configured()),
        }
    }

    pub(in crate::app) fn recording(&self) -> Option<ShortcutRecording> {
        self.recording
    }

    pub(in crate::app) fn is_recording(&self, action: ShortcutAction, global: bool) -> bool {
        self.recording
            == Some(if global {
                ShortcutRecording::Global(action)
            } else {
                ShortcutRecording::Local(action)
            })
    }

    pub(in crate::app) fn start_recording(&mut self, recording: ShortcutRecording) {
        self.clear_recording_messages();
        self.recording = Some(recording);
    }

    /// Return whether the global shortcut backend needs to resume registration.
    pub(in crate::app) fn cancel_recording(&mut self) -> bool {
        let was_recording = self.recording.take().is_some();
        self.clear_recording_messages();
        was_recording
    }

    fn clear_recording_messages(&mut self) {
        self.shortcut_conflict = None;
        self.global_shortcut_conflict = None;
        self.global_shortcut_validation = None;
    }
}

#[cfg(test)]
mod tests;
