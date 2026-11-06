use super::*;

#[test]
fn loading_clamps_equalizer_gains_without_losing_other_config_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    let mut saved = Config {
        server_url: "https://music.example".into(),
        username: "listener".into(),
        password: "secret".into(),
        volume: 0.42,
        ..Default::default()
    };
    saved.equalizer.enabled = true;
    saved.equalizer.gains_db = [-24.0, -12.0, -3.5, 0.0, 4.25, 12.0, 24.0, 1.0, 2.0, 3.0];
    fs::write(&path, serde_json::to_string(&saved).unwrap()).unwrap();
    let restored = read_config_at(&path);
    assert!(restored.equalizer.enabled);
    assert_eq!(
        restored.equalizer.gains_db,
        [-12.0, -12.0, -3.5, 0.0, 4.25, 12.0, 12.0, 1.0, 2.0, 3.0]
    );
    assert_eq!(restored.server_url, saved.server_url);
    assert_eq!(restored.username, saved.username);
    assert_eq!(restored.password, saved.password);
    assert_eq!(restored.volume, saved.volume);
    assert!(restored.active_profile().unwrap().is_configured());
}

#[test]
fn missing_invalid_and_unreadable_settings_restore_a_usable_default_profile() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    let mut expected = Config::default();
    expected.sync_active_profile();
    assert_eq!(
        serde_json::to_value(read_config_at(&path)).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    for text in ["{broken", "[]", "null", r#"{"server_url":false}"#] {
        fs::write(&path, text).unwrap();
        let restored = read_config_at(&path);
        assert_eq!(
            serde_json::to_value(restored).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
    }
    assert_eq!(
        serde_json::to_value(read_config_at(dir.path())).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}

#[test]
fn legacy_global_settings_upgrade_playback_modes_and_normalize_volume() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    for (volume, normalized) in [(-1.0, 0.0), (2.0, 1.0)] {
        let value = serde_json::json!({
            "server_url": "https://music.example",
            "username": "listener",
            "password": "secret",
            "playback_mode": "repeat_one",
            "volume": volume,
        });
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let restored = read_config_at(&path);
        assert_eq!(restored.volume, normalized);
        assert_eq!(
            restored.playback_modes.repeat,
            crate::models::RepeatMode::One
        );
        assert!(!restored.playback_modes.shuffle);
        assert_eq!(restored.active_profile().unwrap().username, "listener");
        assert_eq!(restored.password, "secret");
    }
}
