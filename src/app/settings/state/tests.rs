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

#[test]
fn configured_active_server_has_no_implicit_form_target() {
    assert!(SettingsUi::default()
        .server_form_target(&server_config(true))
        .is_none());
}

#[test]
fn unconfigured_active_server_is_the_implicit_form_target() {
    let config = server_config(false);
    assert_eq!(
        SettingsUi::default().server_form_target(&config),
        config.active_profile()
    );
}

#[test]
fn explicit_edit_target_takes_priority_without_switching_servers() {
    let mut settings = SettingsUi {
        editing_server: Some("other".into()),
        ..Default::default()
    };
    for active_configured in [true, false] {
        let config = server_config(active_configured);
        assert_eq!(
            settings.server_form_target(&config),
            Some(&config.servers[1])
        );
        assert_eq!(config.active_server.as_deref(), Some("active"));
    }
    settings.editing_server = Some("active".into());
    let config = server_config(true);
    assert_eq!(
        settings.server_form_target(&config),
        config.active_profile()
    );
}

#[test]
fn missing_edit_target_never_falls_back_to_the_active_server() {
    let settings = SettingsUi {
        editing_server: Some("removed".into()),
        ..Default::default()
    };
    for active_configured in [true, false] {
        assert!(settings
            .server_form_target(&server_config(active_configured))
            .is_none());
    }
}

#[test]
fn missing_active_server_has_no_implicit_form_target() {
    let mut config = server_config(false);
    config.active_server = Some("removed".into());
    assert!(SettingsUi::default().server_form_target(&config).is_none());
    config.active_server = None;
    assert!(SettingsUi::default().server_form_target(&config).is_none());
    assert!(SettingsUi::default()
        .server_form_target(&Config::default())
        .is_none());
}

#[test]
fn defaults_match_a_closed_server_settings_overlay() {
    let settings = SettingsUi::default();
    assert!(!settings.open);
    assert_eq!(settings.section, SettingsSection::Servers);
    assert_eq!(settings.recording(), None);
    assert!(settings.editing_server.is_none());
    assert!(settings.global_shortcut_failures.is_empty());
}

#[test]
fn changing_recording_target_clears_transient_errors_and_preserves_other_settings() {
    let mut settings = SettingsUi {
        open: true,
        section: SettingsSection::Shortcuts,
        editing_server: Some("another-server".into()),
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
    assert_eq!(settings.editing_server.as_deref(), Some("another-server"));
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
