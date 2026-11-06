use super::*;

#[test]
fn dragging_an_unchanged_band_enables_a_disabled_equalizer() {
    let mut state = EqualizerState::default();
    let mut settings = EqSettings::default();
    let request = state.set_band(&mut settings, 0, 0.0).unwrap();
    assert!(settings.enabled);
    assert_eq!(settings.gains_db, [0.0; 10]);
    assert!(state.complete_save(request));
}

#[test]
fn band_edits_preserve_other_gains_and_do_not_reschedule_unchanged_values() {
    let mut state = EqualizerState::default();
    let mut settings = EqSettings {
        enabled: true,
        gains_db: EqSettings::PRESETS[1].1,
    };
    let mut expected = settings.clone();
    expected.gains_db[4] = -4.0;
    let request = state.set_band(&mut settings, 4, -4.0).unwrap();
    assert_eq!(settings, expected);
    assert!(state.set_band(&mut settings, 4, -4.0).is_none());
    assert!(state.complete_save(request));
}

#[test]
fn out_of_range_band_edits_compare_the_effective_gain() {
    let mut state = EqualizerState::default();
    let mut settings = EqSettings::default();
    let first = state.set_band(&mut settings, 0, 100.0).unwrap();
    assert_eq!(settings.gains_db[0], 12.0);
    assert!(state.set_band(&mut settings, 0, 20.0).is_none());
    assert!(state.complete_save(first));
    let second = state.set_band(&mut settings, 0, -100.0).unwrap();
    assert_eq!(settings.gains_db[0], -12.0);
    assert!(state.complete_save(second));
}

#[test]
fn an_invalid_band_does_not_change_settings_or_cancel_a_pending_save() {
    let mut state = EqualizerState::default();
    let mut settings = EqSettings::default();
    let request = state.set_band(&mut settings, 3, 4.0).unwrap();
    let expected = settings.clone();
    for band in [EqSettings::BANDS.len(), usize::MAX] {
        assert!(state.set_band(&mut settings, band, 10.0).is_none());
        assert_eq!(settings, expected);
    }
    assert!(state.complete_save(request));
}

#[test]
fn invalid_gains_cannot_enter_the_canonical_settings() {
    for gain in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut state = EqualizerState::default();
        let mut settings = EqSettings {
            enabled: true,
            gains_db: [4.0; 10],
        };
        let request = state.set_band(&mut settings, 2, gain).unwrap();
        assert_eq!(settings.gains_db[2], 0.0);
        assert_eq!(settings.gains_db[3], 4.0);
        assert!(settings.gains_db.iter().all(|gain| gain.is_finite()));
        assert!(state.complete_save(request));
    }
}

#[test]
fn toggling_keeps_band_values_and_repeated_toggles_preserve_the_pending_save() {
    let mut state = EqualizerState::default();
    let mut settings = EqSettings {
        enabled: true,
        gains_db: EqSettings::PRESETS[4].1,
    };
    let original = settings.gains_db;
    let disabled = state.set_enabled(&mut settings, false).unwrap();
    assert!(!settings.enabled);
    assert_eq!(settings.gains_db, original);
    assert!(state.set_enabled(&mut settings, false).is_none());
    assert!(state.complete_save(disabled));
    let enabled = state.set_enabled(&mut settings, true).unwrap();
    assert!(settings.enabled);
    assert_eq!(settings.gains_db, original);
    assert!(state.complete_save(enabled));
}

#[test]
fn each_builtin_preset_enables_the_equalizer_and_replaces_all_bands() {
    for (_, preset) in EqSettings::PRESETS {
        let mut state = EqualizerState::default();
        let mut settings = EqSettings {
            enabled: false,
            gains_db: [-10.0; 10],
        };
        let request = state.set_preset(&mut settings, preset).unwrap();
        assert!(settings.enabled);
        assert_eq!(settings.gains_db, preset);
        assert!(state.set_preset(&mut settings, preset).is_none());
        assert!(state.complete_save(request));
    }
}

#[test]
fn preset_inputs_are_normalized_before_storage_and_repeat_comparison() {
    let mut state = EqualizerState::default();
    let mut settings = EqSettings::default();
    let mut gains = [1.5; 10];
    gains[0] = 20.0;
    gains[1] = -20.0;
    gains[2] = f32::NAN;
    gains[3] = f32::INFINITY;
    let request = state.set_preset(&mut settings, gains).unwrap();
    assert_eq!(
        settings.gains_db,
        [12.0, -12.0, 0.0, 0.0, 1.5, 1.5, 1.5, 1.5, 1.5, 1.5]
    );
    assert!(state.set_preset(&mut settings, gains).is_none());
    assert!(state.complete_save(request));
}

#[test]
fn mixed_edits_accept_only_the_latest_save_callback_once() {
    let mut state = EqualizerState::default();
    let mut settings = EqSettings::default();
    let band = state.set_band(&mut settings, 1, 4.0).unwrap();
    let preset = state
        .set_preset(&mut settings, EqSettings::PRESETS[4].1)
        .unwrap();
    let toggle = state.set_enabled(&mut settings, false).unwrap();
    assert!(!state.complete_save(band));
    assert!(!state.complete_save(preset));
    assert!(state.complete_save(toggle));
    assert!(!state.complete_save(toggle));
    let next = state.set_enabled(&mut settings, true).unwrap();
    assert!(!state.complete_save(toggle));
    assert!(state.complete_save(next));
    assert_eq!(settings.gains_db, EqSettings::PRESETS[4].1);
}
