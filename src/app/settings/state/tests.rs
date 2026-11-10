use super::*;

fn server_config(active_configured: bool) -> Config {
    Config {
        active_server: Some("active".into()),
        servers: vec![
            ServerProfile {
                id: "active".into(),
                ..ServerProfile::new(
                    if active_configured {
                        "https://active.example"
                    } else {
                        ""
                    },
                    "listener",
                    "secret",
                )
            },
            ServerProfile {
                id: "other".into(),
                ..ServerProfile::new("https://other.example", "listener", "secret")
            },
        ],
        ..Default::default()
    }
}

fn server_dialog(target: Option<&str>) -> SettingsDialog {
    SettingsDialog::Server(ServerDialog {
        target: target.map(str::to_owned),
        connection: Default::default(),
    })
}

#[test]
fn only_an_open_server_dialog_has_a_form_target() {
    for active_configured in [true, false] {
        let config = server_config(active_configured);
        assert!(SettingsUi::default().server_form_target(&config).is_none());
        let mut settings = SettingsUi::default();
        settings.open_dialog(server_dialog(Some("other")));
        assert_eq!(
            settings.server_form_target(&config),
            Some(&config.servers[1])
        );
        assert_eq!(config.active_server.as_deref(), Some("active"));
        // Adding a server has no saved profile yet.
        settings.open_dialog(server_dialog(None));
        assert!(settings.server_form_target(&config).is_none());
        settings.open_dialog(SettingsDialog::BilibiliLogin);
        assert!(settings.server_form_target(&config).is_none());
    }
}

#[test]
fn missing_edit_target_never_falls_back_to_the_active_server() {
    let mut settings = SettingsUi::default();
    settings.open_dialog(server_dialog(Some("removed")));
    for active_configured in [true, false] {
        assert!(settings
            .server_form_target(&server_config(active_configured))
            .is_none());
    }
}

#[test]
fn the_app_closes_its_dialog_once_and_a_user_close_needs_no_request() {
    let mut settings = SettingsUi::default();
    settings.open_dialog(server_dialog(None));
    assert!(settings.dialog.is_some());
    settings.dismiss_dialog();
    assert!(settings.dialog.is_none());
    assert!(settings.take_close_request());
    assert!(!settings.take_close_request());
    settings.open_dialog(server_dialog(None));
    settings.dialog_closed();
    assert!(settings.dialog.is_none());
    assert!(!settings.take_close_request());
    // Dismissing nothing must not close an unrelated window dialog.
    settings.dismiss_dialog();
    assert!(!settings.take_close_request());
}

#[test]
fn closing_settings_dismisses_its_dialog_and_login() {
    let mut settings = SettingsUi {
        open: true,
        ..Default::default()
    };
    settings.open_dialog(SettingsDialog::BilibiliLogin);
    settings.close();
    assert!(!settings.open);
    assert!(settings.dialog.is_none());
    assert!(settings.take_close_request());
}

#[test]
fn defaults_match_a_closed_server_settings_overlay() {
    let settings = SettingsUi::default();
    assert!(!settings.open);
    assert_eq!(settings.section, SettingsSection::Sources);
    assert_eq!(settings.recording(), None);
    assert!(settings.dialog.is_none());
    assert!(settings.global_shortcut_failures.is_empty());
}

#[test]
fn changing_recording_target_clears_transient_errors_and_preserves_other_settings() {
    let mut settings = SettingsUi {
        open: true,
        section: SettingsSection::Shortcuts,
        global_shortcut_failures: HashSet::from([ShortcutAction::Previous]),
        ..Default::default()
    };
    settings.start_recording(ShortcutRecording::Local(ShortcutAction::Next));
    assert!(settings.is_recording(ShortcutAction::Next, false));
    assert!(!settings.is_recording(ShortcutAction::Next, true));
    settings.shortcut_conflict = Some((ShortcutAction::Next, ShortcutAction::Previous));
    settings.global_shortcut_conflict =
        Some((ShortcutAction::Next, ShortcutAction::TogglePlayback));
    settings.global_shortcut_validation =
        Some((ShortcutAction::Next, ValidationError::UnsafeModifiers));

    settings.start_recording(ShortcutRecording::Global(ShortcutAction::TogglePlayback));
    assert!(!settings.is_recording(ShortcutAction::Next, false));
    assert!(settings.is_recording(ShortcutAction::TogglePlayback, true));
    assert!(settings.shortcut_conflict.is_none());
    assert!(settings.global_shortcut_conflict.is_none());
    assert!(settings.global_shortcut_validation.is_none());
    assert!(settings.open);
    assert_eq!(settings.section, SettingsSection::Shortcuts);
    assert!(settings
        .global_shortcut_failures
        .contains(&ShortcutAction::Previous));
    assert!(settings.cancel_recording());
    assert_eq!(settings.recording(), None);
    assert!(!settings.cancel_recording());
}

#[test]
fn cancelling_idle_errors_does_not_request_global_shortcut_registration() {
    let mut settings = SettingsUi {
        shortcut_conflict: Some((ShortcutAction::Next, ShortcutAction::Previous)),
        global_shortcut_conflict: Some((ShortcutAction::Next, ShortcutAction::TogglePlayback)),
        global_shortcut_validation: Some((ShortcutAction::Next, ValidationError::UnsupportedKey)),
        ..Default::default()
    };
    assert!(!settings.cancel_recording());
    assert!(settings.shortcut_conflict.is_none());
    assert!(settings.global_shortcut_conflict.is_none());
    assert!(settings.global_shortcut_validation.is_none());
    settings.start_recording(ShortcutRecording::Local(ShortcutAction::Next));
    assert!(settings.cancel_recording());
}
