use super::*;

use crate::models::{PlayModes, RepeatMode, Song};

const SERVER_URL: &str = "https://music.example";

fn cache(dir: &Path, server_id: &str) -> ServerCache {
    ServerCache::new(dir.to_path_buf(), Some(server_id))
}

fn session(position: f64) -> PlaybackSession {
    PlaybackSession {
        server_url: SERVER_URL.into(),
        queue: vec![Song {
            id: "song-1".into(),
            ..Default::default()
        }],
        queue_index: Some(0),
        position_secs: position,
        ..Default::default()
    }
}

fn genres() -> Vec<Genre> {
    vec![Genre {
        value: "Rock".into(),
        song_count: Some(120),
        album_count: Some(12),
    }]
}

#[test]
fn scoped_cache_paths_preserve_existing_filenames_for_every_kind() {
    let dir = tempfile::tempdir().unwrap();
    for (server_id, scope) in [(None, "default"), (Some("Srv-A_2"), "srv-a_2")] {
        let cache = ServerCache::new(dir.path().into(), server_id);
        for (kind, stem) in [
            (CacheKind::Session, "session"),
            (CacheKind::Loudness, "loudness"),
            (CacheKind::Genres, "genres"),
            (CacheKind::ListeningHistory, "listening-history"),
            (CacheKind::DailyMix, "daily-mix"),
        ] {
            assert_eq!(
                cache.path(kind),
                dir.path().join(format!("{stem}-{scope}.json"))
            );
            assert_eq!(
                cache.legacy_path(kind),
                dir.path().join(format!("{stem}.json"))
            );
        }
    }
}

#[test]
fn session_migration_keeps_the_source_after_a_failed_write_and_retries() {
    let dir = tempfile::tempdir().unwrap();
    let cache = cache(dir.path(), "srv-a");
    let legacy = dir.path().join("session.json");
    let mut legacy_value = serde_json::to_value(session(42.5)).unwrap();
    legacy_value
        .as_object_mut()
        .unwrap()
        .remove("playback_modes");
    legacy_value["playback_mode"] = serde_json::json!("repeat_one");
    let original = serde_json::to_vec(&legacy_value).unwrap();
    fs::write(&legacy, &original).unwrap();
    let destination = dir.path().join("session-srv-a.json");
    // A directory blocks atomic file replacement on every platform without
    // relying on permissions, process-wide environment changes, or real config.
    fs::create_dir(&destination).unwrap();

    let restored = cache.session(SERVER_URL).unwrap();
    assert_eq!(restored.position_secs, 42.5);
    assert_eq!(restored.playback_modes.repeat, RepeatMode::One);
    assert_eq!(fs::read(&legacy).unwrap(), original);
    assert!(destination.is_dir());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);

    fs::remove_dir(&destination).unwrap();
    let retried = cache.session(SERVER_URL).unwrap();
    assert_eq!(retried.position_secs, 42.5);
    assert!(!legacy.exists());
    let persisted = read_session_at(&destination).unwrap();
    assert_eq!(persisted.queue[0].id, "song-1");
    assert_eq!(persisted.playback_modes.repeat, RepeatMode::One);
    assert_eq!(persisted.position_secs, 42.5);
}

#[test]
fn a_scoped_session_takes_precedence_and_leaves_the_legacy_file_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let cache = cache(dir.path(), "srv-a");
    let legacy = cache.legacy_path(CacheKind::Session);
    write_json_atomic(&legacy, &session(10.0), "test session").unwrap();
    cache.save(CacheKind::Session, &session(70.0)).unwrap();
    assert_eq!(cache.session(SERVER_URL).unwrap().position_secs, 70.0);
    assert_eq!(read_session_at(&legacy).unwrap().position_secs, 10.0);
}

#[test]
fn a_legacy_session_is_adopted_only_by_its_matching_server() {
    let dir = tempfile::tempdir().unwrap();
    let first = cache(dir.path(), "srv-a");
    let second = cache(dir.path(), "srv-b");
    let legacy = dir.path().join("session.json");
    write_json_atomic(&legacy, &session(45.0), "test session").unwrap();
    assert!(second.session("https://other.example").is_none());
    assert!(legacy.exists());
    assert!(!dir.path().join("session-srv-b.json").exists());
    assert_eq!(
        first
            .session("https://music.example/")
            .unwrap()
            .position_secs,
        45.0
    );
    assert!(!legacy.exists());
    assert!(second.session(SERVER_URL).is_none());
}

#[test]
fn legacy_session_adoption_matches_the_restore_address_and_preserves_proxy_mount_case() {
    let dir = tempfile::tempdir().unwrap();
    let cache = cache(dir.path(), "srv-a");
    let legacy = cache.legacy_path(CacheKind::Session);
    let mut saved = session(42.125);
    saved.server_url = "MUSIC.example:80/Music/".into();
    write_json_atomic(&legacy, &saved, "test session").unwrap();
    assert!(cache.session("http://music.example/music").is_none());
    assert!(legacy.exists());
    let restored = cache.session("http://music.example/Music").unwrap();
    assert!(restored.matches_server("http://music.example/Music"));
    assert_eq!(restored.position_secs, 42.125);
    assert!(!legacy.exists());
    let persisted = read_session_at(&cache.path(CacheKind::Session)).unwrap();
    assert!(persisted.matches_server("http://music.example/Music"));
    assert_eq!(persisted.server_url, saved.server_url);
}

#[test]
fn legacy_loudness_is_adopted_once_and_keeps_the_tuple_map_format() {
    let dir = tempfile::tempdir().unwrap();
    let first = cache(dir.path(), "srv-a");
    let second = cache(dir.path(), "srv-b");
    let expected = HashMap::from([("song-1".to_string(), (0.25, 0.75))]);
    let legacy = dir.path().join("loudness.json");
    write_json_atomic(&legacy, &expected, "test loudness").unwrap();
    assert_eq!(first.loudness(), expected);
    assert!(!legacy.exists());
    assert_eq!(
        read_json_at::<serde_json::Value>(&dir.path().join("loudness-srv-a.json")).unwrap(),
        serde_json::json!({"song-1": [0.25, 0.75]})
    );
    assert!(second.loudness().is_empty());
}

#[test]
fn genres_round_trip_and_legacy_adoption_do_not_cross_servers() {
    let dir = tempfile::tempdir().unwrap();
    let first = cache(dir.path(), "srv-a");
    let second = cache(dir.path(), "srv-b");
    let legacy = dir.path().join("genres.json");
    write_json_atomic(&legacy, &genres(), "test genres").unwrap();
    let restored = first.genres().unwrap();
    assert_eq!(restored[0].value, "Rock");
    assert_eq!(restored[0].song_count, Some(120));
    assert!(!legacy.exists());
    assert!(second.genres().is_none());
    first.save(CacheKind::Genres, &[] as &[Genre]).unwrap();
    assert!(first.genres().unwrap().is_empty());
}

#[test]
fn failed_loudness_and_genre_migrations_preserve_the_legacy_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let cache = cache(dir.path(), "srv-a");
    let legacy_loudness = dir.path().join("loudness.json");
    let legacy_genres = dir.path().join("genres.json");
    let expected = HashMap::from([("song-1".to_string(), (0.25, 0.75))]);
    write_json_atomic(&legacy_loudness, &expected, "test loudness").unwrap();
    write_json_atomic(&legacy_genres, &genres(), "test genres").unwrap();
    let loudness_before = fs::read(&legacy_loudness).unwrap();
    let genres_before = fs::read(&legacy_genres).unwrap();
    fs::create_dir(dir.path().join("loudness-srv-a.json")).unwrap();
    fs::create_dir(dir.path().join("genres-srv-a.json")).unwrap();
    assert_eq!(cache.loudness(), expected);
    assert_eq!(cache.genres().unwrap()[0].value, "Rock");
    assert_eq!(fs::read(&legacy_loudness).unwrap(), loudness_before);
    assert_eq!(fs::read(&legacy_genres).unwrap(), genres_before);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 4);
}

#[test]
fn a_corrupt_scoped_file_can_be_replaced_by_a_valid_legacy_cache() {
    let dir = tempfile::tempdir().unwrap();
    let cache = cache(dir.path(), "srv-a");
    let destination = dir.path().join("genres-srv-a.json");
    fs::write(&destination, "{broken").unwrap();
    write_json_atomic(&dir.path().join("genres.json"), &genres(), "test genres").unwrap();
    assert_eq!(cache.genres().unwrap()[0].value, "Rock");
    assert_eq!(
        read_json_at::<Vec<Genre>>(&destination).unwrap()[0].value,
        "Rock"
    );
    assert!(!dir.path().join("genres.json").exists());
}

#[test]
fn missing_or_corrupt_server_caches_fall_back_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    let cache = cache(dir.path(), "srv-a");
    for corrupted in [false, true] {
        if corrupted {
            for kind in CacheKind::ALL {
                fs::write(cache.path(kind), "{broken").unwrap();
                fs::write(cache.legacy_path(kind), "{broken").unwrap();
            }
        }
        assert!(cache.session(SERVER_URL).is_none());
        assert!(cache.genres().is_none());
        assert!(cache.loudness().is_empty());
        assert!(cache
            .load::<ListeningHistory>(CacheKind::ListeningHistory)
            .is_none());
        assert!(cache
            .load::<DailyMixSnapshot>(CacheKind::DailyMix)
            .is_none());
    }
}

#[test]
fn listening_history_round_trips_and_corruption_falls_back() {
    let dir = tempfile::tempdir().unwrap();
    let first = cache(dir.path(), "srv-a");
    let second = cache(dir.path(), "srv-b");
    let mut history = ListeningHistory::default();
    history.record_started(
        Song {
            id: "song-1".into(),
            ..Default::default()
        },
        123,
    );
    assert!(history.update_latest("song-1", 61, true));
    first.save(CacheKind::ListeningHistory, &history).unwrap();
    let restored: ListeningHistory = first.load(CacheKind::ListeningHistory).unwrap();
    assert_eq!(restored.events.len(), 1);
    assert_eq!(restored.events[0].song.id, "song-1");
    assert_eq!(restored.events[0].listened_seconds, 61);
    assert!(restored.events[0].completed);
    assert!(second
        .load::<ListeningHistory>(CacheKind::ListeningHistory)
        .is_none());
    fs::write(dir.path().join("listening-history-srv-a.json"), "{broken").unwrap();
    assert!(first
        .load::<ListeningHistory>(CacheKind::ListeningHistory)
        .is_none());
}

#[test]
fn daily_mix_round_trips_in_its_server_scope() {
    let dir = tempfile::tempdir().unwrap();
    let first = cache(dir.path(), "srv-a");
    let second = cache(dir.path(), "srv-b");
    let snapshot = DailyMixSnapshot {
        server_id: "srv-a".into(),
        day: "2026-11-04".into(),
        algorithm_version: 2,
        variant: 3,
        generated_at: 123,
        tracks: vec![],
    };
    first.save(CacheKind::DailyMix, &snapshot).unwrap();
    let restored: DailyMixSnapshot = first.load(CacheKind::DailyMix).unwrap();
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(snapshot).unwrap()
    );
    assert!(second
        .load::<DailyMixSnapshot>(CacheKind::DailyMix)
        .is_none());
}

#[test]
fn caches_without_a_legacy_format_do_not_adopt_global_files() {
    let dir = tempfile::tempdir().unwrap();
    let cache = cache(dir.path(), "srv-a");
    write_json_atomic(
        &dir.path().join("listening-history.json"),
        &ListeningHistory::default(),
        "test history",
    )
    .unwrap();
    assert!(cache
        .load::<ListeningHistory>(CacheKind::ListeningHistory)
        .is_none());
    assert!(dir.path().join("listening-history.json").exists());
}

#[test]
fn remove_server_cache_deletes_only_that_servers_five_scoped_files() {
    let dir = tempfile::tempdir().unwrap();
    for stem in [
        "session",
        "loudness",
        "genres",
        "listening-history",
        "daily-mix",
    ] {
        fs::write(dir.path().join(format!("{stem}-srv-a.json")), "a").unwrap();
        fs::write(dir.path().join(format!("{stem}-srv-b.json")), "b").unwrap();
        fs::write(dir.path().join(format!("{stem}.json")), "legacy").unwrap();
    }
    fs::write(dir.path().join("config.json"), "config").unwrap();
    fs::create_dir(dir.path().join("covers")).unwrap();
    fs::write(dir.path().join("covers").join("image.cover"), "cover").unwrap();
    assert_eq!(cache(dir.path(), "srv-a").remove(), 5);
    for stem in [
        "session",
        "loudness",
        "genres",
        "listening-history",
        "daily-mix",
    ] {
        assert!(!dir.path().join(format!("{stem}-srv-a.json")).exists());
        assert_eq!(
            fs::read_to_string(dir.path().join(format!("{stem}-srv-b.json"))).unwrap(),
            "b"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(format!("{stem}.json"))).unwrap(),
            "legacy"
        );
    }
    assert!(dir.path().join("config.json").exists());
    assert!(dir.path().join("covers").join("image.cover").exists());
    assert_eq!(cache(dir.path(), "srv-a").remove(), 0);
    assert_eq!(cache(dir.path(), "srv-404").remove(), 0);
    assert_eq!(cache(&dir.path().join("missing"), "srv-a").remove(), 0);
}

#[test]
fn a_scoped_session_upgrades_legacy_playback_modes_without_changing_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let cache = cache(dir.path(), "srv-a");
    let path = cache.path(CacheKind::Session);
    let mut saved = serde_json::to_value(session(42.0)).unwrap();
    saved.as_object_mut().unwrap().remove("playback_modes");
    saved["playback_mode"] = serde_json::json!("shuffle");
    fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
    let original = fs::read(&path).unwrap();
    let restored = cache.session(SERVER_URL).unwrap();
    assert_eq!(
        restored.playback_modes,
        PlayModes {
            shuffle: true,
            repeat: RepeatMode::Off
        }
    );
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn typed_background_snapshots_preserve_all_five_native_json_formats_and_paths() {
    let dir = tempfile::tempdir().unwrap();
    let cache = cache(dir.path(), "srv-a");
    let saved_session = session(42.125);
    let measurements = HashMap::from([("song-1".into(), (-16.0, 0.5))]);
    let genres = genres();
    let mut history = ListeningHistory::default();
    history.record_started(saved_session.queue[0].clone(), 123);
    history.update_latest("song-1", 2, true);
    let daily = DailyMixSnapshot {
        server_id: "srv-a".into(),
        day: "2026-11-04".into(),
        algorithm_version: 2,
        variant: 1,
        generated_at: 123,
        tracks: vec![],
    };
    let cases = [
        (
            "session-srv-a.json",
            CacheSnapshot::Session(saved_session.clone()),
            serde_json::to_value(saved_session).unwrap(),
        ),
        (
            "loudness-srv-a.json",
            CacheSnapshot::Loudness(measurements),
            serde_json::json!({"song-1": [-16.0, 0.5]}),
        ),
        (
            "genres-srv-a.json",
            CacheSnapshot::Genres(genres.clone()),
            serde_json::to_value(genres).unwrap(),
        ),
        (
            "listening-history-srv-a.json",
            CacheSnapshot::ListeningHistory(history.clone()),
            serde_json::to_value(history).unwrap(),
        ),
        (
            "daily-mix-srv-a.json",
            CacheSnapshot::DailyMix(daily.clone()),
            serde_json::to_value(daily).unwrap(),
        ),
    ];
    for (name, snapshot, expected) in cases {
        cache.save_snapshot(&snapshot).unwrap();
        let saved: serde_json::Value = read_json_at(&dir.path().join(name)).unwrap();
        assert_eq!(saved, expected, "native format changed for {name}");
    }
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 5);
}
