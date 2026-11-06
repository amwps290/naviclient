use super::*;

#[test]
fn legacy_configs_keep_the_disabled_flat_default() {
    let config: crate::models::Config = serde_json::from_str(
        r#"{"server_url":"https://music.example","username":"user","password":"secret"}"#,
    )
    .unwrap();
    assert_eq!(config.equalizer, EqSettings::default());
    for json in [r#"{}"#, r#"{"enabled":false}"#] {
        let settings: EqSettings = serde_json::from_str(json).unwrap();
        assert_eq!(settings, EqSettings::default());
    }
}

#[test]
fn settings_keep_the_existing_serialized_shape() {
    let settings = EqSettings {
        enabled: true,
        gains_db: EqSettings::PRESETS[1].1,
    };
    let serialized = serde_json::to_value(&settings).unwrap();
    assert_eq!(
        serialized,
        serde_json::json!({
            "enabled": true,
            "gains_db": [7.0, 6.0, 4.5, 3.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        })
    );
    assert_eq!(
        serde_json::from_value::<EqSettings>(serialized).unwrap(),
        settings
    );
}

#[test]
fn gain_normalization_preserves_valid_bands_and_neutralizes_non_finite_values() {
    let mut settings = EqSettings {
        enabled: false,
        gains_db: [
            -24.0,
            -12.0,
            -3.5,
            0.0,
            4.25,
            12.0,
            24.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ],
    };
    settings.clamp_gains();
    assert!(!settings.enabled);
    assert_eq!(
        settings.gains_db,
        [-12.0, -12.0, -3.5, 0.0, 4.25, 12.0, 12.0, 0.0, 0.0, 0.0]
    );
    let normalized = settings.clone();
    settings.clamp_gains();
    assert_eq!(settings, normalized);
}

#[test]
fn built_in_presets_use_valid_gains_for_every_band() {
    for (name, gains_db) in EqSettings::PRESETS {
        let mut settings = EqSettings {
            enabled: true,
            gains_db,
        };
        settings.clamp_gains();
        assert_eq!(
            settings.gains_db, gains_db,
            "preset {name} needs normalization"
        );
    }
}
