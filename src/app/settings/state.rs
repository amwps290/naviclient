//! Settings overlay state and mutually exclusive shortcut recording sessions.

use std::collections::HashSet;

use crate::global_shortcuts::ValidationError;
use crate::models::network::{NetworkError, SourceConnection};
use crate::models::{Config, ServerProfile, ShortcutAction};
use crate::source::SourceId;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::app) enum SettingsSection {
    #[default]
    Sources,
    Proxies,
    Playback,
    Lyrics,
    Desktop,
    Appearance,
    Shortcuts,
}

impl SettingsSection {
    pub(in crate::app) const ALL: [Self; 7] = [
        Self::Sources,
        Self::Proxies,
        Self::Playback,
        Self::Lyrics,
        Self::Desktop,
        Self::Appearance,
        Self::Shortcuts,
    ];
}

/// Adding or editing a Navidrome server. A new server has no target yet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct ServerDialog {
    pub(in crate::app) target: Option<String>,
    pub(in crate::app) connection: SourceConnection,
}

/// Adding or editing a proxy, with its inline validation error.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct ProxyDialog {
    pub(in crate::app) target: Option<String>,
    pub(in crate::app) error: Option<NetworkError>,
}

/// The one settings dialog that may be open. The window owns the dialog
/// itself; this state decides its content and whether it should stay open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) enum SettingsDialog {
    Server(ServerDialog),
    Proxy(ProxyDialog),
    BilibiliLogin,
    RemoveBilibili(SourceId),
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
    pub(in crate::app) dialog: Option<SettingsDialog>,
    /// Background work finished the dialog; the next render closes it.
    close_requested: bool,
    pub(in crate::app) proxy_probes: super::proxy_probe::ProxyProbes,
}

impl SettingsUi {
    pub(in crate::app) fn close(&mut self) {
        self.open = false;
        self.bilibili_login.cancel();
        self.dismiss_dialog();
    }

    pub(in crate::app) fn open_dialog(&mut self, dialog: SettingsDialog) {
        self.dialog = Some(dialog);
        self.close_requested = false;
    }

    /// The app finished the dialog's task (saved, signed in, removed).
    pub(in crate::app) fn dismiss_dialog(&mut self) {
        if self.dialog.take().is_some() {
            self.close_requested = true;
        }
    }

    /// The user closed the window dialog (Cancel, Escape, close button).
    pub(in crate::app) fn dialog_closed(&mut self) {
        self.dialog = None;
        self.close_requested = false;
    }

    pub(in crate::app) fn take_close_request(&mut self) -> bool {
        std::mem::take(&mut self.close_requested)
    }

    /// The Save action must address the profile shown in the dialog. An
    /// obsolete edit target must never fall back to another server.
    pub(in crate::app) fn server_form_target<'a>(
        &self,
        config: &'a Config,
    ) -> Option<&'a ServerProfile> {
        let Some(SettingsDialog::Server(ServerDialog {
            target: Some(id), ..
        })) = &self.dialog
        else {
            return None;
        };
        config.servers.iter().find(|profile| &profile.id == id)
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
