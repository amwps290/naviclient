//! Settings overlay state and mutually exclusive shortcut recording sessions.

use std::collections::HashSet;

use crate::global_shortcuts::ValidationError;
use crate::models::{Config, ServerProfile, ShortcutAction};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SettingsSection {
    #[default]
    Servers,
    Playback,
    Desktop,
    Appearance,
    Shortcuts,
}

impl SettingsSection {
    pub(super) const ALL: [Self; 5] = [
        Self::Servers,
        Self::Playback,
        Self::Desktop,
        Self::Appearance,
        Self::Shortcuts,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ShortcutRecording {
    Local(ShortcutAction),
    Global(ShortcutAction),
}

#[derive(Default)]
pub(super) struct SettingsUi {
    pub(super) open: bool,
    pub(super) section: SettingsSection,
    recording: Option<ShortcutRecording>,
    /// The action being edited and the action already using its binding.
    pub(super) shortcut_conflict: Option<(ShortcutAction, ShortcutAction)>,
    pub(super) global_shortcut_conflict: Option<(ShortcutAction, ShortcutAction)>,
    pub(super) global_shortcut_validation: Option<(ShortcutAction, ValidationError)>,
    /// Registration failures survive changes of the recording target.
    pub(super) global_shortcut_failures: HashSet<ShortcutAction>,
    /// Editing another profile does not switch the active server.
    /// Reconnecting resets the entire settings state and closes this form.
    pub(super) editing_server: Option<String>,
}

impl SettingsUi {
    /// The visible form and Save/Cancel actions must address the same profile.
    /// An obsolete explicit edit target must never fall back to another server.
    pub(super) fn server_form_target<'a>(&self, config: &'a Config) -> Option<&'a ServerProfile> {
        match self.editing_server.as_deref() {
            Some(id) => config.servers.iter().find(|profile| profile.id == id),
            None => config
                .active_profile()
                .filter(|profile| !profile.is_configured()),
        }
    }

    pub(super) fn recording(&self) -> Option<ShortcutRecording> {
        self.recording
    }

    pub(super) fn is_recording(&self, action: ShortcutAction, global: bool) -> bool {
        self.recording
            == Some(if global {
                ShortcutRecording::Global(action)
            } else {
                ShortcutRecording::Local(action)
            })
    }

    pub(super) fn start_recording(&mut self, recording: ShortcutRecording) {
        self.clear_recording_messages();
        self.recording = Some(recording);
    }

    /// Return whether the global shortcut backend needs to resume registration.
    pub(super) fn cancel_recording(&mut self) -> bool {
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
