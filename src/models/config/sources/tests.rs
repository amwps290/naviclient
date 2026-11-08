use super::*;
use crate::models::ServerProfile;

fn bilibili(id: &str) -> SourceProfile {
    SourceProfile::Bilibili {
        id: id.into(),
        name: "Bilibili".into(),
        account_id: None,
        credential_key: None,
        proxy: None,
    }
}

fn with_servers() -> Config {
    let mut config = Config {
        servers: vec![
            ServerProfile::new("https://one.example", "alice", "secret-a"),
            ServerProfile::new("https://two.example", "bob", "secret-b"),
        ],
        ..Config::default()
    };
    config.sync_active_profile();
    config
}

#[test]
fn legacy_server_migration_preserves_ids_and_avoids_duplicate_credentials() {
    let mut config: Config = serde_json::from_str(
        r#"{"server_url":"https://music.example","username":"alice","password":"secret"}"#,
    )
    .unwrap();
    config.sync_active_profile();
    let server_id = config.servers[0].id.clone();
    assert_eq!(
        config.sources,
        vec![SourceProfile::OpenSubsonic {
            server_id: server_id.clone()
        }]
    );
    assert_eq!(
        config.active_source,
        Some(SourceId::new(ProviderKind::OpenSubsonic, &server_id))
    );
    let source_json = serde_json::to_string(&config.sources).unwrap();
    assert!(!source_json.contains("secret"));
    assert!(!source_json.contains("password"));
    assert_eq!(config.password, config.servers[0].password);
}

#[test]
fn source_migration_is_idempotent_across_persistence_and_server_edits() {
    let mut config = with_servers();
    let sources = config.sources.clone();
    let id = config.servers[0].id.clone();
    config.store_active("https://renamed.example", "robert", "new-secret");
    config.servers[0].name = "Renamed".into();
    config.servers[0].password.clear();
    config.servers[0].password_in_keyring = true;
    config.sync_active_profile();
    for _ in 0..3 {
        config = serde_json::from_value(serde_json::to_value(config).unwrap()).unwrap();
        config.sync_active_profile();
        assert_eq!(config.sources, sources);
        assert_eq!(config.servers[0].id, id);
        assert_eq!(config.server_url, "https://renamed.example");
        assert!(config.password.is_empty());
        assert!(config.servers[0].password_in_keyring);
    }
}

#[test]
fn bilibili_only_config_needs_no_placeholder_server_or_legacy_fields() {
    let mut config: Config =
        serde_json::from_str(r#"{"sources":[{"provider":"bilibili","id":"account-profile"}]}"#)
            .unwrap();
    for _ in 0..3 {
        config.sync_active_profile();
        assert!(config.servers.is_empty());
        assert!(config.active_server.is_none());
        assert_eq!(
            config.active_source,
            Some(SourceId::new(ProviderKind::Bilibili, "account-profile"))
        );
        assert_eq!(config.sources.len(), 1);
    }
}

#[test]
fn bilibili_selection_does_not_overwrite_existing_server_mirrors() {
    let mut config = with_servers();
    config.sources.push(bilibili("bili-account"));
    let server = config.servers[0].clone();
    let source = SourceId::new(ProviderKind::Bilibili, "bili-account");
    assert!(config.switch_source(&source));
    config.sync_active_profile();
    assert_eq!(config.active_source, Some(source));
    assert_eq!(config.active_server.as_deref(), Some(server.id.as_str()));
    assert_eq!(config.server_url, server.url);
    assert_eq!(config.username, server.username);
    assert_eq!(config.password, server.password);
    let next = config.servers[1].clone();
    assert!(config.switch_source(&SourceId::new(ProviderKind::OpenSubsonic, &next.id)));
    assert_eq!(config.server_url, next.url);
    assert_eq!(config.username, next.username);
    assert_eq!(config.password, next.password);
}

#[test]
fn source_selection_is_canonical_when_legacy_active_server_disagrees() {
    let mut config = with_servers();
    let next = config.servers[1].clone();
    config.active_source = Some(SourceId::new(ProviderKind::OpenSubsonic, &next.id));
    config.sync_active_profile();
    assert_eq!(config.active_server.as_deref(), Some(next.id.as_str()));
    assert_eq!(config.server_url, next.url);
    assert_eq!(config.username, next.username);
}

#[test]
fn equal_profile_ids_across_providers_survive_source_deduplication() {
    let mut config = with_servers();
    let server_id = config.servers[0].id.clone();
    config.sources.push(bilibili(&server_id));
    config.sources.push(bilibili(&server_id));
    config.sources.push(SourceProfile::OpenSubsonic {
        server_id: server_id.clone(),
    });
    config.sources.push(SourceProfile::OpenSubsonic {
        server_id: "deleted".into(),
    });
    config.sync_active_profile();
    assert_eq!(config.sources.len(), 3);
    assert!(config.switch_source(&SourceId::new(ProviderKind::Bilibili, server_id)));
}

#[test]
fn unknown_source_selection_preserves_all_existing_settings() {
    let mut config = with_servers();
    let before = serde_json::to_value(&config).unwrap();
    assert!(!config.switch_source(&SourceId::new(ProviderKind::Bilibili, "missing")));
    assert_eq!(serde_json::to_value(&config).unwrap(), before);
    config.active_source = Some(SourceId::new(ProviderKind::Bilibili, "missing"));
    let active = config.active_server.clone().unwrap();
    config.sync_active_profile();
    assert_eq!(
        config.active_source,
        Some(SourceId::new(ProviderKind::OpenSubsonic, active))
    );
}

#[test]
fn server_addition_and_removal_keep_source_references_in_sync() {
    let mut config = with_servers();
    let added = config.add_server();
    assert_eq!(config.sources.len(), 3);
    assert_eq!(
        config.active_source,
        Some(SourceId::new(ProviderKind::OpenSubsonic, &added))
    );
    assert!(config.remove_server(&added));
    assert_eq!(config.sources.len(), 2);
    assert!(!config
        .sources
        .iter()
        .any(|source| source.id().profile_id == added));
}

#[test]
fn removing_servers_while_browsing_bilibili_retains_that_source() {
    let mut config = with_servers();
    let first = config.servers[0].id.clone();
    let second = config.servers[1].id.clone();
    config.sources.push(bilibili("bili-account"));
    let source = SourceId::new(ProviderKind::Bilibili, "bili-account");
    assert!(config.switch_source(&source));
    assert!(config.remove_server(&first));
    assert_eq!(config.active_source, Some(source.clone()));
    assert!(config.remove_server(&second));
    assert!(config.servers.is_empty());
    assert!(config.active_server.is_none());
    assert!(config.server_url.is_empty());
    assert!(config.username.is_empty());
    assert!(config.password.is_empty());
    assert_eq!(config.active_source, Some(source));
    config.sync_active_profile();
    assert!(config.servers.is_empty());
}

#[test]
fn mixed_legacy_credentials_and_bilibili_profiles_retain_both_sources() {
    let mut config: Config = serde_json::from_str(
        r#"{"server_url":"https://music.example","username":"alice","password":"secret", "sources":[{"provider":"bilibili","id":"account"}], "active_source":{"provider":"bilibili","profile_id":"account"}}"#,
    ).unwrap();
    config.sync_active_profile();
    assert_eq!(config.sources.len(), 2);
    assert_eq!(config.servers.len(), 1);
    assert_eq!(
        config.active_source,
        Some(SourceId::new(ProviderKind::Bilibili, "account"))
    );
    assert_eq!(config.username, "alice");
}

#[test]
fn invalid_blank_bilibili_profile_does_not_disable_legacy_default_recovery() {
    let mut config = Config {
        sources: vec![bilibili(" ")],
        ..Config::default()
    };
    config.sync_active_profile();
    assert_eq!(config.servers.len(), 1);
    assert_eq!(config.sources.len(), 1);
    assert_eq!(
        config.active_source.as_ref().unwrap().provider,
        ProviderKind::OpenSubsonic
    );
}

#[test]
fn damaged_source_fields_keep_valid_legacy_servers_and_preferences() {
    let config = with_servers();
    let original = serde_json::to_value(&config).unwrap();
    for sources in [
        serde_json::json!([{"provider":"future-provider","id":"unknown"},
            {"provider":"bilibili","id":"valid-bili"},
            {"provider":"bilibili","id":false}]),
        serde_json::json!("damaged source list"),
        serde_json::Value::Null,
    ] {
        let mut value = original.clone();
        value["sources"] = sources;
        value["active_source"] =
            serde_json::json!({"provider":"unsupported","profile_id":"missing"});
        value["volume"] = serde_json::json!(0.42);
        let mut restored: Config = serde_json::from_value(value).unwrap();
        restored.sync_active_profile();
        assert_eq!(restored.servers, config.servers);
        assert_eq!(restored.active_server, config.active_server);
        assert_eq!(restored.volume, 0.42);
        assert_eq!(restored.password, config.password);
        assert_eq!(restored.active_source, config.active_source);
    }
}
#[test]
fn browsing_switch_preserves_an_independent_playback_source_and_invalid_owner_is_removed() {
    let mut config = Config::default();
    config.sync_active_profile();
    let playing = config.active_source.clone().unwrap();
    config.playback_source = Some(playing.clone());
    config.sources.push(SourceProfile::Bilibili {
        id: "account".into(),
        name: String::new(),
        account_id: None,
        credential_key: None,
        proxy: None,
    });
    assert!(config.switch_source(&SourceId::new(ProviderKind::Bilibili, "account")));
    config.sync_active_profile();
    assert_eq!(config.playback_source, Some(playing));
    let restored: Config = serde_json::from_value(serde_json::to_value(&config).unwrap()).unwrap();
    assert_eq!(restored.playback_source, config.playback_source);
    config.playback_source = Some(SourceId::new(ProviderKind::Bilibili, "deleted"));
    config.sync_active_profile();
    assert!(config.playback_source.is_none());
}
