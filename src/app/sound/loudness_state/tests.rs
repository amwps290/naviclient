use super::*;

fn song(id: &str) -> Song {
    Song {
        id: id.into(),
        ..Default::default()
    }
}

#[test]
fn replay_gain_applies_db_gain_in_track_mode() {
    // -3.5 dB ≈ 0.668；0 dB = 1.0；+6 dB ≈ 1.995
    let factor = replay_gain_factor(VolumeNormalization::Track, Some(-3.5), None, None, None);
    assert!((factor - 10f32.powf(-3.5 / 20.0)).abs() < 0.001);
    assert_eq!(
        replay_gain_factor(VolumeNormalization::Track, Some(0.0), None, None, None),
        1.0
    );
    let boost = replay_gain_factor(VolumeNormalization::Track, Some(6.0), None, None, None);
    assert!((boost - 10f32.powf(6.0 / 20.0)).abs() < 0.001);
}

#[test]
fn replay_gain_album_mode_uses_album_values() {
    let factor = replay_gain_factor(VolumeNormalization::Album, None, None, Some(-2.0), None);
    assert!((factor - 10f32.powf(-2.0 / 20.0)).abs() < 0.001);
    // Track 模式下 album 增益不生效。
    assert_eq!(
        replay_gain_factor(VolumeNormalization::Track, None, None, Some(-2.0), None),
        1.0
    );
}

#[test]
fn replay_gain_off_ignores_metadata_and_prevents_clipping() {
    assert_eq!(
        replay_gain_factor(VolumeNormalization::Off, Some(-8.0), None, None, None),
        1.0
    );
    // 缺失增益数据时安全回退为 1.0。
    assert_eq!(
        replay_gain_factor(VolumeNormalization::Track, None, Some(0.9), None, None),
        1.0
    );
    // 峰值防削波：增益被限制在 1/peak。
    let clamped = replay_gain_factor(
        VolumeNormalization::Track,
        Some(10.0),
        Some(0.8),
        None,
        None,
    );
    assert!((clamped - 1.25).abs() < 0.001);
    // 结果始终落在安全范围内。
    assert_eq!(
        replay_gain_factor(VolumeNormalization::Track, Some(100.0), None, None, None),
        4.0
    );
}

#[test]
fn measured_loudness_maps_rms_to_gain() {
    // 目标 -16 dBFS：rms -12 → 增益 -4 dB ≈ 0.631；rms -24 → +8 dB ≈ 2.512
    let loud = loudness_gain_factor(-12.0, 0.0);
    assert!((loud - 10f32.powf(-4.0 / 20.0)).abs() < 0.001);
    let quiet = loudness_gain_factor(-24.0, 0.0);
    assert!((quiet - 10f32.powf(8.0 / 20.0)).abs() < 0.001);
    // 峰值防削波：峰值 0.9 时增益不超过 1/0.9。
    let clamped = loudness_gain_factor(-24.0, 0.9);
    assert!((clamped - (1.0 / 0.9)).abs() < 0.001);
}

#[test]
fn selected_server_tags_take_priority_over_local_measurements() {
    let mut cache = LoudnessCache::default();
    assert!(cache.record(VolumeNormalization::Track, "song", -24.0, 0.1));
    let song = Song {
        replay_gain_track_gain: Some(6.0),
        replay_gain_album_gain: Some(-6.0),
        ..song("song")
    };

    assert!((cache.gain(&song, VolumeNormalization::Track) - 10f32.powf(6.0 / 20.0)).abs() < 0.001);
    assert!(
        (cache.gain(&song, VolumeNormalization::Album) - 10f32.powf(-6.0 / 20.0)).abs() < 0.001
    );
}

#[test]
fn selected_peak_limits_the_selected_gain() {
    let cache = LoudnessCache::default();
    let song = Song {
        replay_gain_track_gain: Some(12.0),
        replay_gain_track_peak: Some(0.8),
        replay_gain_album_gain: Some(12.0),
        replay_gain_album_peak: Some(0.4),
        ..song("song")
    };
    assert!((cache.gain(&song, VolumeNormalization::Track) - 1.25).abs() < 0.001);
    assert!((cache.gain(&song, VolumeNormalization::Album) - 2.5).abs() < 0.001);
}

#[test]
fn missing_selected_tags_use_local_measurements_without_borrowing_the_other_mode() {
    let mut cache = LoudnessCache::default();
    cache.record(VolumeNormalization::Track, "song", -24.0, 0.9);
    let local_gain = 1.0 / 0.9;
    let only_album_tag = Song {
        replay_gain_album_gain: Some(-6.0),
        ..song("song")
    };
    let only_track_tag = Song {
        replay_gain_track_gain: Some(-6.0),
        ..song("song")
    };

    assert!((cache.gain(&only_album_tag, VolumeNormalization::Track) - local_gain).abs() < 0.001);
    assert!((cache.gain(&only_track_tag, VolumeNormalization::Album) - local_gain).abs() < 0.001);
    assert_eq!(
        cache.gain(&song("missing"), VolumeNormalization::Track),
        1.0
    );
    assert_eq!(
        cache.gain(&song("missing"), VolumeNormalization::Album),
        1.0
    );
}

#[test]
fn zero_server_gain_is_present_and_does_not_fall_back_to_local_gain() {
    let mut cache = LoudnessCache::default();
    cache.record(VolumeNormalization::Track, "song", -24.0, 0.1);
    let song = Song {
        replay_gain_track_gain: Some(0.0),
        replay_gain_album_gain: Some(0.0),
        ..song("song")
    };
    assert_eq!(cache.gain(&song, VolumeNormalization::Track), 1.0);
    assert_eq!(cache.gain(&song, VolumeNormalization::Album), 1.0);
}

#[test]
fn disabled_normalization_neither_uses_nor_replaces_measurements() {
    let mut cache = LoudnessCache::default();
    cache.record(VolumeNormalization::Track, "song", -24.0, 0.1);
    let song = Song {
        replay_gain_track_gain: Some(6.0),
        ..song("song")
    };

    assert_eq!(cache.gain(&song, VolumeNormalization::Off), 1.0);
    assert!(!cache.record(VolumeNormalization::Off, "song", -12.0, 0.8));
    assert_eq!(cache.measurement("song"), Some((-24.0, 0.1)));
}

#[test]
fn invalid_measurements_preserve_existing_entries_and_their_json_roundtrip() {
    let mut cache = LoudnessCache::default();
    cache.record(VolumeNormalization::Track, "song", -24.0, 0.9);
    for (rms_db, peak) in [
        (0.0, 0.9),
        (f32::NAN, 0.9),
        (f32::INFINITY, 0.9),
        (-24.0, f32::NAN),
        (-24.0, f32::INFINITY),
        (-24.0, -0.1),
    ] {
        assert!(!cache.record(VolumeNormalization::Track, "song", rms_db, peak));
    }
    assert!(!cache.record(VolumeNormalization::Track, "  ", -24.0, 0.9));
    let json = serde_json::to_string(cache.items()).unwrap();
    let restored: HashMap<String, (f32, f32)> = serde_json::from_str(&json).unwrap();
    assert_eq!(&restored, cache.items());
    assert_eq!(restored.get("song"), Some(&(-24.0, 0.9)));
    assert_eq!(restored.len(), 1);
}

#[test]
fn restoration_keeps_valid_legacy_measurements_and_discards_bad_entries() {
    let mut cache = LoudnessCache::default();
    cache.restore(
        Some("server"),
        HashMap::from([
            ("valid".into(), (-24.0, 0.9)),
            ("no-peak".into(), (-24.0, 0.0)),
            ("unmeasured".into(), (0.0, 0.9)),
            ("bad-peak".into(), (-24.0, f32::NAN)),
            ("bad-rms".into(), (f32::NAN, 0.9)),
            ("".into(), (-24.0, 0.9)),
        ]),
    );
    assert_eq!(cache.items().len(), 2);
    assert_eq!(cache.measurement("valid"), Some((-24.0, 0.9)));
    assert_eq!(cache.measurement("no-peak"), Some((-24.0, 0.0)));
    assert!(
        (cache.gain(&song("no-peak"), VolumeNormalization::Track) - 10f32.powf(8.0 / 20.0)).abs()
            < 0.001
    );
}

#[test]
fn late_measurements_stay_with_the_original_server_until_restoration() {
    let mut config = crate::models::Config::default();
    let first = config.add_server();
    let second = config.add_server();
    assert!(config.switch_to(&first));
    let mut cache = LoudnessCache::default();
    cache.restore(config.active_server_id(), HashMap::new());
    cache.record(VolumeNormalization::Track, "old-only", -24.0, 0.9);
    assert!(config.switch_to(&second));

    assert!(cache.record(VolumeNormalization::Track, "shared-id", -24.0, 0.9));
    assert_eq!(cache.server_id(), Some(first.as_str()));
    assert_eq!(config.active_server_id(), Some(second.as_str()));
    cache.restore(
        config.active_server_id(),
        HashMap::from([("shared-id".into(), (-12.0, 0.8))]),
    );
    assert_eq!(cache.server_id(), Some(second.as_str()));
    assert_eq!(cache.measurement("shared-id"), Some((-12.0, 0.8)));
    assert_eq!(cache.measurement("old-only"), None);
    assert!(
        (cache.gain(&song("shared-id"), VolumeNormalization::Track) - 10f32.powf(-4.0 / 20.0))
            .abs()
            < 0.001
    );
}

#[test]
fn cache_snapshots_keep_the_existing_song_id_to_pair_format() {
    let legacy: HashMap<String, (f32, f32)> =
        serde_json::from_str(r#"{"song":[-24.0,0.9]}"#).unwrap();
    let mut cache = LoudnessCache::default();
    cache.restore(Some("server"), legacy.clone());
    let encoded = serde_json::to_string(cache.items()).unwrap();
    let restored: HashMap<String, (f32, f32)> = serde_json::from_str(&encoded).unwrap();
    assert_eq!(restored, legacy);
    assert_eq!(cache.server_id(), Some("server"));
}
