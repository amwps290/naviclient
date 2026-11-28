use crate::backup::{
    apply_config_backup, apply_equalizer_backup, build_bundle, read_bundle,
    sanitize_config_for_backup, write_bundle, write_safety_snapshot, BackupSelection, RestoreMode,
};
use crate::listening_stats::ListeningLog;
use crate::local_playlists::LocalPlaylistStore;
use crate::lyrics::cache::LyricsCache;
use crate::lyrics::Document;
use crate::models::network::ProxyProfile;
use crate::models::{
    Config, CustomEqPreset, ListeningEvent, Lyrics, ServerProfile, Song, ThemePreference,
};
use crate::source::{MediaKey, ProviderKind, SourceId, SourceProfile};

#[test]
fn export_strips_all_passwords_keyring_flags_bilibili_sessions_and_proxy_userinfo() {
    let mut config = Config {
        password: "legacy-secret".to_string(),
        servers: vec![ServerProfile {
            id: "srv-1".to_string(),
            name: "Home".to_string(),
            url: "https://music.example.com".to_string(),
            username: "alice".to_string(),
            password: "subsonic-password".to_string(),
            password_in_keyring: true,
        }],
        sources: vec![
            SourceProfile::WebDav {
                id: "wd-1".to_string(),
                name: "NAS".to_string(),
                base_url: "https://nas.example.com/dav".to_string(),
                root_path: "/music".to_string(),
                username: "bob".to_string(),
                password: "webdav-password".to_string(),
                password_in_keyring: true,
            },
            SourceProfile::Bilibili {
                id: "bili-1".to_string(),
                name: "Bilibili".to_string(),
                account_id: Some(12345),
                credential_key: Some("secret-cookie-key".to_string()),
                proxy: None,
            },
        ],
        ..Default::default()
    };
    config.network.proxies.push(ProxyProfile {
        id: "p1".to_string(),
        name: "Home Proxy".to_string(),
        url: "http://user:topsecret@127.0.0.1:7890".to_string(),
    });

    let sanitized = sanitize_config_for_backup(&config);
    assert!(sanitized.password.is_empty());
    assert!(sanitized.servers[0].password.is_empty());
    assert!(!sanitized.servers[0].password_in_keyring);
    match &sanitized.sources[0] {
        SourceProfile::WebDav {
            password,
            password_in_keyring,
            ..
        } => {
            assert!(password.is_empty());
            assert!(!*password_in_keyring);
        }
        other => panic!("expected WebDav, got {other:?}"),
    }
    match &sanitized.sources[1] {
        SourceProfile::Bilibili {
            account_id,
            credential_key,
            ..
        } => {
            assert_eq!(*account_id, None);
            assert_eq!(*credential_key, None);
        }
        other => panic!("expected Bilibili, got {other:?}"),
    }
    assert!(!sanitized.network.proxies[0].url.contains("topsecret"));
    assert!(sanitized.network.proxies[0].url.contains("127.0.0.1:7890"));
}

#[test]
fn full_bundle_roundtrip_merge_overwrite_and_safety_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let lyrics_cache = LyricsCache::at(dir.path().join("lyrics-v1.json"));
    let mut playlists = LocalPlaylistStore::open(dir.path().join("playlists"));
    let mut log = ListeningLog::open(dir.path().join("listening-log.jsonl"));

    let source_id = SourceId::new(ProviderKind::LocalFolder, "local-1");
    let song = Song {
        source: Some(source_id.clone()),
        id: "song-1".to_string(),
        title: "Midnight".to_string(),
        artist: "Synth".to_string(),
        duration: Some(210),
        ..Default::default()
    };

    // Populate lyrics preference, playlist, listening log, and EQ preset.
    let mut doc = Document::native(Lyrics::default());
    doc.offset_ms = 450;
    doc.plain = true;
    lyrics_cache
        .remember(&MediaKey::new(source_id.clone(), "song-1"), &doc)
        .unwrap();
    playlists.create("Night Mix", vec![song.clone()]).unwrap();
    log.record_event(&ListeningEvent {
        song: song.clone(),
        started_at: 1_760_000_000,
        listened_seconds: 180,
        completed: true,
    })
    .unwrap();

    let mut cfg = Config {
        theme: ThemePreference::Dark,
        ..Default::default()
    };
    cfg.equalizer.custom_presets.push(CustomEqPreset {
        name: "Studio".to_string(),
        preamp_db: 1.5,
        gains_db: [0.0; 10],
    });
    cfg.servers.push(ServerProfile {
        id: "srv-1".to_string(),
        name: "My Server".to_string(),
        url: "http://nas:4533".to_string(),
        username: "u".to_string(),
        password: "pw".to_string(),
        password_in_keyring: false,
    });

    let bundle = build_bundle(
        BackupSelection::default(),
        &cfg,
        &lyrics_cache,
        &playlists,
        &log,
    );
    let out_path = dir.path().join("export.navibackup.json");
    write_bundle(&out_path, &bundle).unwrap();
    let loaded = read_bundle(&out_path).unwrap();

    assert_eq!(loaded.schema_version, crate::backup::BACKUP_SCHEMA_VERSION);
    assert_eq!(loaded.local_playlists.as_ref().unwrap().len(), 1);
    assert_eq!(loaded.listening_log.as_ref().unwrap().len(), 1);
    assert_eq!(loaded.lyrics_preferences.as_ref().unwrap().songs.len(), 1);

    // Restore onto a second clean environment with an existing server credential.
    let dir2 = tempfile::tempdir().unwrap();
    let lyrics2 = LyricsCache::at(dir2.path().join("lyrics-v1.json"));
    let mut playlists2 = LocalPlaylistStore::open(dir2.path().join("playlists"));
    let mut log2 = ListeningLog::open(dir2.path().join("listening-log.jsonl"));

    let mut existing_cfg = Config::default();
    existing_cfg.servers.push(ServerProfile {
        id: "srv-1".to_string(),
        name: "My Server".to_string(),
        url: "http://nas:4533".to_string(),
        username: "u".to_string(),
        password: "kept-local-secret".to_string(),
        password_in_keyring: true,
    });
    existing_cfg.equalizer.custom_presets.push(CustomEqPreset {
        name: "Local Only".to_string(),
        preamp_db: -1.0,
        gains_db: [0.0; 10],
    });

    let snap = write_safety_snapshot(
        &dir2.path().join("backups"),
        &existing_cfg,
        &lyrics2,
        &playlists2,
        &log2,
    )
    .unwrap();
    assert!(snap.is_file());

    let restored_cfg = apply_config_backup(
        &existing_cfg,
        loaded.config.as_ref().unwrap(),
        RestoreMode::Overwrite,
    );
    // Matching server retains its local secret even under overwrite!
    assert_eq!(restored_cfg.servers[0].password, "kept-local-secret");
    assert!(restored_cfg.servers[0].password_in_keyring);
    assert_eq!(restored_cfg.theme, ThemePreference::Dark);

    let merged_eq = apply_equalizer_backup(
        &existing_cfg.equalizer,
        loaded.equalizer.as_ref().unwrap(),
        RestoreMode::Merge,
    );
    assert_eq!(merged_eq.custom_presets.len(), 2);

    let pl_count = playlists2
        .import_playlists(loaded.local_playlists.unwrap(), false)
        .unwrap();
    assert_eq!(pl_count, 1);
    assert_eq!(playlists2.list().len(), 1);

    let lyr_count = lyrics2
        .import_preferences(loaded.lyrics_preferences.unwrap(), false)
        .unwrap();
    assert_eq!(lyr_count, 1);
    let restored_pref = lyrics2.preference(&MediaKey::new(source_id, "song-1"));
    assert_eq!(restored_pref.offset_ms, 450);

    let log_count = log2
        .import_entries(loaded.listening_log.unwrap(), false)
        .unwrap();
    assert_eq!(log_count, 1);
}
