use std::path::PathBuf;

use super::listening::LISTENING_HISTORY_LIMIT;
use super::preferences::locale_from_os_tag;
use super::{
    AlbumSort, ArtistSort, CapabilitySupport, Config, LanguagePreference, ListeningHistory,
    OpenSubsonicExtension, PlayModes, PlaybackMode, RepeatMode, ServerCapabilities, ServerInfo,
    ServerProfile, ShortcutAction, Song, ThemePreference, TranscodingQuality, VolumeNormalization,
};

#[test]
fn listening_updates_use_source_identity_even_when_another_sources_same_id_is_newer() {
    use crate::source::{MediaKey, ProviderKind, SourceId};
    let first = SourceId::new(ProviderKind::OpenSubsonic, "same-profile");
    let second = SourceId::new(ProviderKind::Bilibili, "same-profile");
    let mut history = ListeningHistory::default();
    for source in [&first, &second] {
        history.record_started(
            Song {
                source: Some(source.clone()),
                id: "same-id".into(),
                ..Song::default()
            },
            123,
        );
    }
    assert!(history.update_latest_media(&MediaKey::new(first, "same-id"), 42, true));
    assert_eq!(history.events[0].listened_seconds, 42);
    assert!(history.events[0].completed);
    assert_eq!(history.events[1].listened_seconds, 0);
    assert!(!history.events[1].completed);
}

#[test]
fn listening_history_tracks_attempts_updates_and_recent_unique_ids() {
    let song = |id: &str| Song {
        id: id.into(),
        title: id.into(),
        ..Song::default()
    };
    let mut history = ListeningHistory::default();
    history.record_started(song(""), 1);
    history.record_started(song("a"), 10);
    history.record_started(song("b"), 20);
    history.record_started(song("a"), 30);

    assert_eq!(history.events.len(), 3);
    assert!(history.update_latest("a", 42, false));
    assert!(history.update_latest("a", 12, true));
    assert!(!history.update_latest("missing", 9, true));
    assert_eq!(history.events[0].listened_seconds, 0);
    assert_eq!(history.events[2].listened_seconds, 42);
    assert!(history.events[2].completed);
    assert_eq!(
        history
            .recent_songs(10)
            .iter()
            .map(|song| song.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b", "a"]
    );
    assert_eq!(
        history.recent_song_ids(10),
        std::collections::HashSet::from(["a".to_string(), "b".to_string()])
    );
    assert!(history.recent_song_ids(0).is_empty());
}

#[test]
fn listening_history_discards_the_oldest_entries_at_its_limit() {
    let mut history = ListeningHistory::default();
    for index in 0..=LISTENING_HISTORY_LIMIT {
        history.record_started(
            Song {
                id: format!("song-{index}"),
                ..Song::default()
            },
            index as u64,
        );
    }
    assert_eq!(history.events.len(), LISTENING_HISTORY_LIMIT);
    assert_eq!(history.events[0].song.id, "song-1");
    assert_eq!(
        history.events.last().map(|event| event.song.id.as_str()),
        Some("song-1000")
    );
}

#[test]
fn capability_matrix_distinguishes_supported_unsupported_and_unknown() {
    let discovered = ServerInfo {
        protocol_version: "1.16.1".into(),
        open_subsonic: true,
        extensions_discovered: true,
        extensions: vec![
            OpenSubsonicExtension {
                name: "songLyrics".into(),
                versions: vec![1, 2],
            },
            OpenSubsonicExtension {
                name: "sonicSimilarity".into(),
                versions: vec![1],
            },
            OpenSubsonicExtension {
                name: "playbackReport".into(),
                versions: vec![1],
            },
        ],
        ..ServerInfo::default()
    };
    let capabilities = ServerCapabilities::from_server_info(&discovered);

    assert_eq!(capabilities.structured_lyrics, CapabilitySupport::Supported);
    assert_eq!(capabilities.enhanced_lyrics, CapabilitySupport::Supported);
    assert_eq!(capabilities.sonic_similarity, CapabilitySupport::Supported);
    assert_eq!(
        capabilities.top_songs_by_artist_id,
        CapabilitySupport::Unsupported
    );
    assert_eq!(capabilities.playback_report, CapabilitySupport::Supported);
    assert_eq!(
        capabilities.index_based_queue,
        CapabilitySupport::Unsupported
    );
    assert_eq!(capabilities.internet_radio, CapabilitySupport::Supported);
    assert_eq!(capabilities.podcasts, CapabilitySupport::Supported);
    assert_eq!(
        capabilities.podcast_episode_lookup,
        CapabilitySupport::Unsupported
    );
    assert_eq!(capabilities.smart_discovery(), CapabilitySupport::Supported);

    let compatibility = ServerInfo {
        protocol_version: "1.8.0".into(),
        ..ServerInfo::default()
    };
    let capabilities = ServerCapabilities::from_server_info(&compatibility);
    assert_eq!(capabilities.structured_lyrics, CapabilitySupport::Unknown);
    assert!(capabilities.structured_lyrics.allows_optimistic_request());
    assert_eq!(capabilities.internet_radio, CapabilitySupport::Unsupported);
    assert_eq!(capabilities.podcasts, CapabilitySupport::Supported);
}

#[test]
fn invalid_protocol_versions_leave_base_capabilities_unknown() {
    let info = ServerInfo {
        protocol_version: "unknown".into(),
        ..ServerInfo::default()
    };
    let capabilities = ServerCapabilities::from_server_info(&info);

    assert_eq!(capabilities.internet_radio, CapabilitySupport::Unknown);
    assert_eq!(capabilities.podcasts, CapabilitySupport::Unknown);
}

#[test]
fn legacy_config_defaults_to_light_theme() {
    let config: Config = serde_json::from_str(
        r#"{"server_url":"http://localhost:4533","username":"user","password":"pass"}"#,
    )
    .expect("legacy config should still deserialize");

    assert_eq!(config.theme, ThemePreference::Light);
    assert_eq!(config.cache_dir, None);
    assert_eq!(config.transcoding_quality, TranscodingQuality::Original);
    assert_eq!(config.volume_normalization, VolumeNormalization::Track);
    assert!((config.volume - 0.7).abs() < f32::EPSILON);
    // 新增的桌面行为字段带默认值，旧配置无需迁移即可加载。
    assert!(config.show_track_notifications);
    assert!(!config.close_minimizes);
    assert_eq!(
        config.shortcuts.binding(ShortcutAction::TogglePlayback),
        "space"
    );
    assert_eq!(
        config
            .global_shortcuts
            .binding(ShortcutAction::TogglePlayback),
        "ctrl-alt-space"
    );
}

#[test]
fn partial_shortcut_config_falls_back_to_new_action_defaults() {
    let config: Config = serde_json::from_str(
        r#"{
                "server_url":"http://localhost:4533",
                "username":"user",
                "password":"pass",
                "shortcuts":{"bindings":{"toggle_playback":"ctrl-p"}}
            }"#,
    )
    .expect("partial shortcut config should deserialize");

    assert_eq!(
        config.shortcuts.binding(ShortcutAction::TogglePlayback),
        "ctrl-p"
    );
    assert_eq!(config.shortcuts.binding(ShortcutAction::Next), "ctrl-right");
}

#[test]
fn partial_global_shortcut_config_falls_back_to_safe_defaults() {
    let config: Config = serde_json::from_str(
        r#"{
                "server_url":"http://localhost:4533",
                "username":"user",
                "password":"pass",
                "global_shortcuts":{"bindings":{"toggle_playback":"win-p"}}
            }"#,
    )
    .expect("partial global shortcut config should deserialize");

    assert_eq!(
        config
            .global_shortcuts
            .binding(ShortcutAction::TogglePlayback),
        "win-p"
    );
    assert_eq!(
        config.global_shortcuts.binding(ShortcutAction::Next),
        "ctrl-alt-right"
    );
}

#[test]
fn song_parses_replay_gain_fields_tolerantly() {
    let song: Song = serde_json::from_str(
        r#"{
                "id": "s1", "title": "Track",
                "replayGainTrackGain": -3.5,
                "replayGainTrackPeak": "0.98",
                "replayGainAlbumGain": -2.1,
                "replayGainAlbumPeak": 1.0,
                "replayGainWeird": "not-a-number"
            }"#,
    )
    .expect("song should parse despite mixed replay gain types");

    assert_eq!(song.replay_gain_track_gain, Some(-3.5));
    assert_eq!(song.replay_gain_track_peak, Some(0.98));
    assert_eq!(song.replay_gain_album_gain, Some(-2.1));
    assert_eq!(song.replay_gain_album_peak, Some(1.0));
}

#[test]
fn volume_normalization_round_trips_through_config_json() {
    let config = Config {
        volume_normalization: VolumeNormalization::Album,
        ..Config::default()
    };
    let text = serde_json::to_string(&config).expect("config should serialize");
    let parsed: Config = serde_json::from_str(&text).expect("config should round-trip");
    assert_eq!(parsed.volume_normalization, VolumeNormalization::Album);
}

#[test]
fn transcoding_quality_uses_supported_subsonic_rates() {
    assert_eq!(TranscodingQuality::Original.max_bit_rate(), None);
    assert_eq!(TranscodingQuality::Kbps192.max_bit_rate(), Some(192));
    assert_eq!(TranscodingQuality::Kbps320.max_bit_rate(), Some(320));
}

#[test]
fn playback_settings_round_trip_through_json() {
    let config = Config {
        cache_dir: Some(PathBuf::from("D:/Music Cache")),
        transcoding_quality: TranscodingQuality::Kbps192,
        volume: 0.42,
        ..Config::default()
    };

    let json = serde_json::to_string(&config).unwrap();
    let restored: Config = serde_json::from_str(&json).unwrap();

    assert_eq!(restored.cache_dir, config.cache_dir);
    assert_eq!(restored.transcoding_quality, TranscodingQuality::Kbps192);
    assert!((restored.volume - 0.42).abs() < f32::EPSILON);
}

#[test]
fn theme_preference_uses_stable_config_names() {
    let config = Config {
        theme: ThemePreference::System,
        ..Config::default()
    };

    let json = serde_json::to_string(&config).expect("config should serialize");

    assert!(json.contains(r#""theme":"system""#));
}

#[test]
fn legacy_playback_mode_maps_to_independent_shuffle_and_repeat() {
    for (legacy, shuffle, repeat) in [
        (PlaybackMode::Sequential, false, RepeatMode::Off),
        (PlaybackMode::RepeatAll, false, RepeatMode::All),
        (PlaybackMode::RepeatOne, false, RepeatMode::One),
        (PlaybackMode::Shuffle, true, RepeatMode::Off),
    ] {
        let modes = PlayModes::from_legacy(legacy);
        assert_eq!(modes.shuffle, shuffle);
        assert_eq!(modes.repeat, repeat);
    }
    assert_eq!(
        PlayModes::default(),
        PlayModes {
            shuffle: false,
            repeat: RepeatMode::Off
        }
    );
}

#[test]
fn repeat_mode_cycles_off_all_one() {
    assert_eq!(RepeatMode::Off.next(), RepeatMode::All);
    assert_eq!(RepeatMode::All.next(), RepeatMode::One);
    assert_eq!(RepeatMode::One.next(), RepeatMode::Off);
}

#[test]
fn legacy_config_defaults_to_sequential_playback_modes() {
    let config: Config = serde_json::from_str(
        r#"{"server_url":"http://localhost:4533","username":"user","password":"pass"}"#,
    )
    .expect("legacy config without playback modes should still deserialize");

    assert_eq!(
        config.playback_modes,
        PlayModes {
            shuffle: false,
            repeat: RepeatMode::Off
        }
    );
}

#[test]
fn auto_continue_is_opt_in_and_round_trips() {
    let legacy: Config = serde_json::from_str(
        r#"{"server_url":"http://localhost:4533","username":"user","password":"pass"}"#,
    )
    .expect("legacy config without auto continue should still deserialize");
    assert!(!legacy.auto_continue);

    let json = serde_json::to_string(&Config {
        auto_continue: true,
        ..Config::default()
    })
    .expect("config should serialize");
    let restored: Config = serde_json::from_str(&json).expect("config should deserialize");
    assert!(restored.auto_continue);
}

#[test]
fn legacy_config_defaults_to_default_library_sorts() {
    let config: Config = serde_json::from_str(
        r#"{"server_url":"http://localhost:4533","username":"user","password":"pass"}"#,
    )
    .expect("legacy config without sort fields should still deserialize");

    assert_eq!(config.albums_sort, AlbumSort::NewestAdded);
    assert_eq!(config.artists_sort, ArtistSort::Name);
}

#[test]
fn output_device_defaults_to_following_system_default() {
    let config: Config = serde_json::from_str(
        r#"{"server_url":"http://localhost:4533","username":"user","password":"pass"}"#,
    )
    .expect("legacy config without output device should still deserialize");
    assert_eq!(config.output_device, None);

    let json = serde_json::to_string(&Config {
        output_device: Some("Speakers".to_string()),
        ..Config::default()
    })
    .expect("config should serialize");
    let restored: Config = serde_json::from_str(&json).expect("config should deserialize");
    assert_eq!(restored.output_device.as_deref(), Some("Speakers"));
}

#[test]
fn legacy_config_defaults_to_showing_lyrics_translation() {
    let config: Config = serde_json::from_str(
        r#"{"server_url":"http://localhost:4533","username":"user","password":"pass"}"#,
    )
    .expect("legacy config without lyrics settings should still deserialize");
    assert!(config.show_lyrics_translation);

    let json = serde_json::to_string(&Config {
        show_lyrics_translation: false,
        ..Config::default()
    })
    .expect("config should serialize");
    let restored: Config = serde_json::from_str(&json).expect("config should deserialize");
    assert!(!restored.show_lyrics_translation);
    assert!(json.contains("show_lyrics_translation"));
}

#[test]
fn interface_language_round_trips_and_legacy_configs_follow_the_system() {
    let config = Config {
        language: LanguagePreference::ChineseTraditional,
        ..Config::default()
    };
    let json = serde_json::to_string(&config).expect("config should serialize");
    let restored: Config = serde_json::from_str(&json).expect("config should deserialize");
    assert_eq!(restored.language, LanguagePreference::ChineseTraditional);
    assert!(json.contains(r#""language":"chinese_traditional""#));

    // 旧配置没有 language 字段：默认 System（跟随操作系统），不能报错。
    let legacy: Config = serde_json::from_str(
        r#"{"server_url":"http://localhost:4533","username":"user","password":"pass"}"#,
    )
    .expect("legacy config without a language should still deserialize");
    assert_eq!(legacy.language, LanguagePreference::System);
    assert_eq!(legacy.language.locale_code(), "system");
}

#[test]
fn os_language_tags_resolve_to_a_supported_catalog() {
    for tag in ["en", "en-US", "de-DE", "ja-JP", "", "  "] {
        assert_eq!(locale_from_os_tag(tag), "en", "tag={tag:?}");
    }
    for tag in ["zh", "zh-CN", "zh-Hans", "zh-Hans-CN", "zh-SG", " ZH-cn "] {
        assert_eq!(locale_from_os_tag(tag), "zh-CN", "tag={tag:?}");
    }
    for tag in [
        "zh-TW",
        "zh-HK",
        "zh-MO",
        "zh-Hant",
        "zh-Hant-TW",
        "zh-hant-hk",
    ] {
        assert_eq!(locale_from_os_tag(tag), "zh-TW", "tag={tag:?}");
    }
    // 显式选择的语言不受操作系统影响。
    assert_eq!(LanguagePreference::English.resolve(), "en");
    assert_eq!(LanguagePreference::ChineseSimplified.resolve(), "zh-CN");
    assert_eq!(LanguagePreference::ChineseTraditional.resolve(), "zh-TW");
    // 语言按钮用各自的母语自称，不随界面语言变化。
    assert_eq!(
        LanguagePreference::ChineseSimplified.native_label(),
        "简体中文"
    );
    assert_eq!(
        LanguagePreference::ChineseTraditional.native_label(),
        "繁體中文"
    );
    assert_eq!(LanguagePreference::English.native_label(), "English");
}

#[test]
fn library_sorts_round_trip_through_config_json() {
    let config = Config {
        albums_sort: AlbumSort::Year,
        artists_sort: ArtistSort::AlbumCount,
        ..Config::default()
    };

    let json = serde_json::to_string(&config).expect("config should serialize");
    let restored: Config = serde_json::from_str(&json).expect("config should deserialize");

    assert_eq!(restored.albums_sort, AlbumSort::Year);
    assert_eq!(restored.artists_sort, ArtistSort::AlbumCount);
    assert!(json.contains(r#""albums_sort":"year""#));
    assert!(json.contains(r#""artists_sort":"album_count""#));
}

#[test]
fn playback_modes_round_trip_through_config_json() {
    let config = Config {
        playback_modes: PlayModes {
            shuffle: true,
            repeat: RepeatMode::One,
        },
        ..Config::default()
    };

    let json = serde_json::to_string(&config).expect("config should serialize");
    let restored: Config = serde_json::from_str(&json).expect("config should deserialize");

    assert!(restored.playback_modes.shuffle);
    assert_eq!(restored.playback_modes.repeat, RepeatMode::One);
    assert!(json.contains(r#""playback_modes":{"shuffle":true,"repeat":"one"}"#));
}

#[test]
fn legacy_single_server_config_migrates_into_the_server_list() {
    let mut config: Config = serde_json::from_str(
        r#"{"server_url":"http://nas.local:4533","username":"alice","password":"secret"}"#,
    )
    .expect("legacy config should deserialize");
    assert!(config.servers.is_empty(), "legacy config has no list yet");

    config.sync_active_profile();

    assert_eq!(config.servers.len(), 1);
    let profile = config.servers.first().unwrap();
    assert_eq!(profile.url, "http://nas.local:4533");
    assert_eq!(profile.username, "alice");
    // 遗留字段保持为激活服务器的镜像，旧调用点无需改动。
    assert_eq!(config.server_url, profile.url);
    assert_eq!(config.active_server.as_deref(), Some(profile.id.as_str()));
}

#[test]
fn server_ids_are_stable_and_independent_of_case_and_trailing_slash() {
    let a = ServerProfile::id_for("http://NAS.local:4533/", "alice");
    let b = ServerProfile::id_for("  http://nas.local:4533  ", " alice ");
    assert_eq!(a, b);
    assert_eq!(a.len(), 12);
    assert!(a.chars().all(|ch| ch.is_ascii_hexdigit()));
    // 不同的用户名/端口必须区分开，否则缓存会串服务器。
    assert_ne!(a, ServerProfile::id_for("http://nas.local:4533", "bob"));
    assert_ne!(a, ServerProfile::id_for("http://nas.local:4534", "alice"));
}

#[test]
fn switching_servers_mirrors_the_active_profile_into_legacy_fields() {
    let mut config = Config {
        servers: vec![
            ServerProfile::new("http://one.local:4533", "alice", "pw1"),
            ServerProfile::new("http://two.local:4533", "bob", "pw2"),
        ],
        ..Config::default()
    };
    config.sync_active_profile();
    let second = config.servers[1].id.clone();

    assert!(config.switch_to(&second));
    assert_eq!(config.server_url, "http://two.local:4533");
    assert_eq!(config.username, "bob");
    assert_eq!(config.password, "pw2");

    // 未知 id 不改变任何状态。
    assert!(!config.switch_to("does-not-exist"));
    assert_eq!(config.username, "bob");
}

#[test]
fn adding_a_server_activates_a_blank_entry_without_colliding_ids() {
    let mut config = Config::default();
    config.sync_active_profile();
    let first = config.active_server.clone().unwrap();

    let added = config.add_server();
    assert_ne!(added, first);
    assert_eq!(config.servers.len(), 2);
    assert_eq!(config.active_server.as_deref(), Some(added.as_str()));
    // 新条目是空白条目，等待用户填写。
    assert!(config.server_url.is_empty());
    assert!(config.username.is_empty());
    assert!(!config.active_profile().unwrap().is_configured());

    // 连续新增不能产生重复 id。
    let another = config.add_server();
    assert_eq!(
        config
            .servers
            .iter()
            .filter(|profile| profile.id == another)
            .count(),
        1
    );
}

#[test]
fn removing_the_active_server_falls_back_to_a_neighbour() {
    let mut config = Config {
        servers: vec![
            ServerProfile::new("http://one.local:4533", "alice", "pw1"),
            ServerProfile::new("http://two.local:4533", "bob", "pw2"),
        ],
        ..Config::default()
    };
    config.sync_active_profile();
    let first = config.servers[0].id.clone();
    let second = config.servers[1].id.clone();
    config.switch_to(&first);

    assert!(config.remove_server(&first));
    assert_eq!(config.servers.len(), 1);
    assert_eq!(config.active_server.as_deref(), Some(second.as_str()));
    assert_eq!(config.server_url, "http://two.local:4533");

    // 最后一条不能删：否则会退化成没有服务器可连的状态。
    assert!(!config.remove_server(&second));
    assert_eq!(config.servers.len(), 1);
}

#[test]
fn editing_the_active_server_keeps_its_id_so_caches_survive_a_url_edit() {
    let mut config = Config {
        servers: vec![ServerProfile::new("http://old.local:4533", "alice", "pw1")],
        ..Config::default()
    };
    config.sync_active_profile();
    let original_id = config.active_server.clone().unwrap();

    let id = config.store_active("http://new.local:4533", "alice", "pw2");

    assert_eq!(id, original_id, "改地址不应换缓存命名空间");
    assert_eq!(config.servers.len(), 1);
    assert_eq!(config.server_url, "http://new.local:4533");
    assert_eq!(config.password, "pw2");
}

#[test]
fn storing_a_known_peer_identity_merges_instead_of_duplicating() {
    let mut config = Config {
        servers: vec![
            ServerProfile::new("http://one.local:4533", "alice", "pw1"),
            ServerProfile::new("http://two.local:4533", "bob", "pw2"),
        ],
        ..Config::default()
    };
    config.sync_active_profile();
    let second = config.servers[1].id.clone();
    config.switch_to(&second);

    // 在第二条条目里填上第一条的地址+账号：应合并回第一条，而不是新建。
    let id = config.store_active("http://one.local:4533", "alice", "pw-new");

    assert_eq!(config.servers.len(), 2);
    assert_eq!(id, config.servers[0].id);
    assert_eq!(config.active_server.as_deref(), Some(id.as_str()));
    assert_eq!(config.servers[0].password, "pw-new");
}

#[test]
fn storing_a_non_active_profile_updates_it_in_place_without_switching() {
    let mut config = Config {
        servers: vec![
            ServerProfile::new("http://one.local:4533", "alice", "pw1"),
            ServerProfile::new("http://two.local:4533", "bob", "pw2"),
        ],
        ..Config::default()
    };
    config.sync_active_profile();
    assert_eq!(
        config.active_server.as_deref(),
        Some(config.servers[0].id.as_str())
    );
    let second_id = config.servers[1].id.clone();

    // 行内铅笔编辑非激活条目：原位更新，id 与激活项都不变。
    assert!(config.store_profile(
        &second_id,
        "  http://two.local:4533/new  ",
        "  robert ",
        "pw-new"
    ));

    let second = &config.servers[1];
    assert_eq!(second.id, second_id, "条目 id 不能变，缓存命名空间依赖它");
    assert_eq!(second.url, "http://two.local:4533/new");
    assert_eq!(second.username, "robert");
    assert_eq!(second.password, "pw-new");
    assert!(!second.password_in_keyring);
    assert_eq!(
        config.active_server.as_deref(),
        Some(config.servers[0].id.as_str()),
        "编辑非激活条目不能把连接切走"
    );

    // 目标 id 不存在（刚被删除）：安全失败，不动任何条目。
    assert!(!config.store_profile("missing", "http://x", "y", "z"));
}

#[test]
fn server_label_falls_back_to_host_when_unnamed() {
    let profile = ServerProfile::new("http://192.168.1.10:4533", "alice", "pw");
    assert_eq!(profile.host(), "192.168.1.10:4533");
    assert_eq!(profile.label(), "192.168.1.10:4533");

    let named = ServerProfile {
        name: "  Home NAS  ".to_string(),
        ..profile
    };
    assert_eq!(named.label(), "Home NAS");
}

#[test]
fn keyring_flag_defaults_off_for_legacy_configs() {
    let config: Config = serde_json::from_str(
        r#"{"server_url":"http://nas.local:4533","username":"alice","password":"secret","servers":[{"id":"abc","url":"http://nas.local:4533","username":"alice","password":"secret"}]}"#,
    )
    .expect("legacy server entries should deserialize without the keyring flag");
    assert!(!config.servers[0].password_in_keyring);

    // 序列化时必须写出该标记，供回环保持。
    let json = serde_json::to_string(&config).unwrap();
    assert!(json.contains("password_in_keyring"));
}

#[test]
fn resolved_password_prefers_the_credential_store_and_falls_back_to_plaintext() {
    let profile = ServerProfile::new("http://nas.local:4533", "alice", "pw");
    assert_eq!(profile.resolved_password(), "pw");

    // 凭据库里没有这个 id 的条目（测试专用 id）：即使被误标为已迁入，
    // 也必须回退明文字段，绝不能因为读取失败而丢密码。
    let flagged = ServerProfile {
        password_in_keyring: true,
        ..profile
    };
    assert_eq!(flagged.resolved_password(), "pw");
}

#[test]
fn opus_sources_fall_back_to_server_transcoding_only_at_original_quality() {
    // Original + Opus：自动降级到既有档位里最高的 320 kbps。
    assert_eq!(
        TranscodingQuality::effective_for(
            TranscodingQuality::Original,
            Some("opus"),
            Some("audio/ogg")
        ),
        TranscodingQuality::transcode_fallback()
    );
    // 判定容忍大小写与空白；content type audio/opus 单独成立。
    assert!(TranscodingQuality::is_undecodable_source(
        Some(" OPUS "),
        None
    ));
    assert!(TranscodingQuality::is_undecodable_source(
        None,
        Some("Audio/Opus")
    ));

    // 已选转码档位：服务器本来就会转码，保持用户选择不变。
    assert_eq!(
        TranscodingQuality::effective_for(TranscodingQuality::Kbps128, Some("opus"), None),
        TranscodingQuality::Kbps128
    );

    // Vorbis / FLAC / 未知后缀都保持 Original：只有 Opus 才降级，
    // 尤其不能把 Ogg Vorbis（可正常解码）一并误伤。
    for suffix in ["ogg", "oga", "flac", "mp3", "m4a", "wma"] {
        assert!(!TranscodingQuality::is_undecodable_source(
            Some(suffix),
            None
        ));
        assert_eq!(
            TranscodingQuality::effective_for(TranscodingQuality::Original, Some(suffix), None),
            TranscodingQuality::Original
        );
    }

    // 两者都缺失时不盲目降级（无从判定，保持现状）。
    assert!(!TranscodingQuality::is_undecodable_source(None, None));
    assert_eq!(
        TranscodingQuality::effective_for(TranscodingQuality::Original, None, None),
        TranscodingQuality::Original
    );
}
