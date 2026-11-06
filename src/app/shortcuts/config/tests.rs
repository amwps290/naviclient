use super::*;
use crate::models::{ServerProfile, ThemePreference};

fn custom_config(sparse: bool) -> Config {
    let mut config = Config {
        server_url: "https://music.example/subsonic".into(),
        username: "listener".into(),
        password: "unchanged".into(),
        servers: vec![ServerProfile::new(
            "https://other.example",
            "another-listener",
            "another-secret",
        )],
        active_server: Some("preserved-server".into()),
        theme: ThemePreference::Dark,
        volume: 0.42,
        cache_dir: Some("/custom/audio-cache".into()),
        output_device: Some("custom-device".into()),
        ..Default::default()
    };
    config.shortcuts.set(ShortcutAction::Next, "ctrl-n".into());
    config
        .global_shortcuts
        .set(ShortcutAction::Next, "ctrl-alt-n".into());
    if sparse {
        config.shortcuts.bindings.remove(&ShortcutAction::Previous);
        config
            .global_shortcuts
            .bindings
            .remove(&ShortcutAction::Previous);
    }
    config
}

fn cases(original: &Config) -> [(ShortcutConfigEdit, Config); 6] {
    let mut local_binding = original.clone();
    local_binding
        .shortcuts
        .bindings
        .insert(ShortcutAction::Previous, "ctrl-p".into());
    let mut global_binding = original.clone();
    global_binding
        .global_shortcuts
        .bindings
        .insert(ShortcutAction::Previous, "ctrl-alt-p".into());
    let mut local_reset = original.clone();
    local_reset.shortcuts.reset(ShortcutAction::Next);
    let mut global_reset = original.clone();
    global_reset.global_shortcuts.reset(ShortcutAction::Next);
    let mut local_defaults = original.clone();
    local_defaults.shortcuts = ShortcutSettings::default();
    let mut global_defaults = original.clone();
    global_defaults.global_shortcuts = GlobalShortcutSettings::default();
    [
        (
            ShortcutConfigEdit::LocalBinding(ShortcutAction::Previous, "ctrl-p".into()),
            local_binding,
        ),
        (
            ShortcutConfigEdit::GlobalBinding(ShortcutAction::Previous, "ctrl-alt-p".into()),
            global_binding,
        ),
        (
            ShortcutConfigEdit::LocalDefault(ShortcutAction::Next),
            local_reset,
        ),
        (
            ShortcutConfigEdit::GlobalDefault(ShortcutAction::Next),
            global_reset,
        ),
        (ShortcutConfigEdit::LocalDefaults, local_defaults),
        (ShortcutConfigEdit::GlobalDefaults, global_defaults),
    ]
}

#[test]
fn successful_edits_save_once_and_change_only_the_selected_bindings() {
    for sparse in [false, true] {
        let original = custom_config(sparse);
        for (edit, expected) in cases(&original) {
            let mut config = original.clone();
            let expected = serde_json::to_value(expected).unwrap();
            let mut saves = 0;
            edit.save(&mut config, |updated| {
                saves += 1;
                assert_eq!(serde_json::to_value(updated).unwrap(), expected);
                Ok(())
            })
            .unwrap();
            assert_eq!(saves, 1);
            assert_eq!(serde_json::to_value(config).unwrap(), expected);
        }
    }
}

#[test]
fn failed_edits_restore_exact_maps_and_keep_the_original_error() {
    for sparse in [false, true] {
        let original = custom_config(sparse);
        for (edit, expected) in cases(&original) {
            let mut config = original.clone();
            let mut saves = 0;
            let error = edit
                .save(&mut config, |updated| {
                    saves += 1;
                    assert_eq!(
                        serde_json::to_value(updated).unwrap(),
                        serde_json::to_value(expected).unwrap()
                    );
                    Err(anyhow::Error::new(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "read-only preferences",
                    ))
                    .context("shortcut persistence failed"))
                })
                .unwrap_err();
            assert_eq!(saves, 1);
            assert_eq!(
                serde_json::to_value(config).unwrap(),
                serde_json::to_value(&original).unwrap()
            );
            assert_eq!(
                format!("{error:#}"),
                "shortcut persistence failed: read-only preferences"
            );
            assert_eq!(
                error.downcast_ref::<std::io::Error>().unwrap().kind(),
                std::io::ErrorKind::PermissionDenied
            );
        }
    }
}

#[test]
fn restoring_individual_defaults_matches_model_reset_for_every_action() {
    for action in ShortcutAction::ALL {
        let mut config = custom_config(true);
        config.shortcuts.set(action, "ctrl-shift-x".into());
        let mut expected = config.clone();
        expected.shortcuts.reset(action);
        ShortcutConfigEdit::LocalDefault(action)
            .save(&mut config, |_| Ok(()))
            .unwrap();
        assert_eq!(config.shortcuts, expected.shortcuts);
        assert_eq!(config.global_shortcuts, expected.global_shortcuts);
    }
    for action in ShortcutAction::GLOBAL_PLAYBACK {
        let mut config = custom_config(true);
        config.global_shortcuts.set(action, "ctrl-alt-x".into());
        let mut expected = config.clone();
        expected.global_shortcuts.reset(action);
        ShortcutConfigEdit::GlobalDefault(action)
            .save(&mut config, |_| Ok(()))
            .unwrap();
        assert_eq!(config.global_shortcuts, expected.global_shortcuts);
        assert_eq!(config.shortcuts, expected.shortcuts);
    }
}

#[test]
fn restoring_missing_defaults_materializes_only_the_requested_binding() {
    for action in ShortcutAction::ALL {
        let mut config = custom_config(true);
        config.shortcuts.bindings.clear();
        let global = config.global_shortcuts.clone();
        ShortcutConfigEdit::LocalDefault(action)
            .save(&mut config, |_| Ok(()))
            .unwrap();
        assert_eq!(config.shortcuts.bindings.len(), 1);
        assert_eq!(config.shortcuts.bindings[&action], action.default_binding());
        assert_eq!(config.global_shortcuts, global);
    }
    for action in ShortcutAction::GLOBAL_PLAYBACK {
        let mut config = custom_config(true);
        config.global_shortcuts.bindings.clear();
        let local = config.shortcuts.clone();
        ShortcutConfigEdit::GlobalDefault(action)
            .save(&mut config, |_| Ok(()))
            .unwrap();
        assert_eq!(config.global_shortcuts.bindings.len(), 1);
        assert_eq!(
            config.global_shortcuts.bindings[&action],
            action.default_global_binding().unwrap()
        );
        assert_eq!(config.shortcuts, local);
    }
}

#[test]
fn successful_edit_after_failed_save_uses_restored_configuration() {
    let mut config = custom_config(true);
    let previous = config.shortcuts.clone();
    assert!(
        ShortcutConfigEdit::LocalBinding(ShortcutAction::Previous, "ctrl-p".into())
            .save(&mut config, |_| Err(anyhow::anyhow!("write failed")))
            .is_err()
    );
    assert_eq!(config.shortcuts, previous);

    ShortcutConfigEdit::GlobalBinding(ShortcutAction::Previous, "ctrl-alt-p".into())
        .save(&mut config, |updated| {
            assert_eq!(updated.shortcuts, previous);
            assert_eq!(
                updated.global_shortcuts.binding(ShortcutAction::Previous),
                "ctrl-alt-p"
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(config.shortcuts, previous);
}
