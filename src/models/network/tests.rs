use super::*;
use crate::models::source::ProviderKind;

fn bilibili(id: &str) -> SourceId {
    SourceId::new(ProviderKind::Bilibili, id)
}

fn subsonic(config: &Config) -> SourceId {
    config
        .sources
        .iter()
        .find(|profile| matches!(profile, SourceProfile::OpenSubsonic { .. }))
        .unwrap()
        .id()
}

/// A configuration saved before shared routing existed: Bilibili sources
/// either carry an inline proxy or connect directly, OpenSubsonic follows
/// the system (environment) proxy.
fn legacy() -> Config {
    let mut config = Config::default();
    config.sync_active_profile();
    config.network = NetworkSettings::legacy();
    for (id, proxy) in [
        ("a", Some("http://127.0.0.1:7890")),
        ("b", Some("http://127.0.0.1:7890")),
        ("c", Some("http://127.0.0.1:7891")),
        ("direct", None),
    ] {
        config.sources.push(SourceProfile::Bilibili {
            id: id.into(),
            name: id.into(),
            account_id: Some(1),
            credential_key: Some(format!("credential-{id}")),
            proxy: proxy.map(str::to_owned),
        });
    }
    config
}

#[test]
fn upgrade_keeps_each_source_on_its_previous_route_and_is_idempotent() {
    let mut config = legacy();
    config.active_source = Some(bilibili("a"));
    config.playback_source = Some(bilibili("b"));
    let original = config.clone();
    config.sync_network();
    assert_eq!(config.network.proxies.len(), 2);
    for (id, expected) in [
        ("a", NetworkRoute::Proxy("http://127.0.0.1:7890".into())),
        ("b", NetworkRoute::Proxy("http://127.0.0.1:7890".into())),
        ("c", NetworkRoute::Proxy("http://127.0.0.1:7891".into())),
        // Released Bilibili sources never used the environment proxy.
        ("direct", NetworkRoute::Direct),
    ] {
        assert_eq!(config.route_for(&bilibili(id)), Ok(expected), "{id}");
    }
    // Released OpenSubsonic clients used reqwest's environment proxy.
    assert_eq!(
        config.route_for(&subsonic(&config)),
        Ok(NetworkRoute::System)
    );
    assert_eq!(config.active_source, original.active_source);
    assert_eq!(config.playback_source, original.playback_source);
    for profile in &config.sources {
        if let SourceProfile::Bilibili { proxy, .. } = profile {
            assert!(proxy.is_none(), "legacy inline proxy is migrated");
        }
    }
    let serialized = serde_json::to_value(&config).unwrap();
    let mut restored: Config = serde_json::from_value(serialized.clone()).unwrap();
    restored.sync_active_profile();
    assert_eq!(serde_json::to_value(&restored).unwrap(), serialized);
}

#[test]
fn sources_added_after_the_upgrade_follow_the_system_proxy() {
    let mut config = legacy();
    config.sync_network();
    config.sources.push(SourceProfile::Bilibili {
        id: "new".into(),
        name: String::new(),
        account_id: None,
        credential_key: None,
        proxy: None,
    });
    config.sync_network();
    assert_eq!(config.route_for(&bilibili("new")), Ok(NetworkRoute::System));
    assert_eq!(
        config.source_connection(&bilibili("new")),
        SourceConnection::System
    );
}

#[test]
fn a_fresh_configuration_routes_new_bilibili_sources_through_the_system_proxy() {
    let mut config = Config::default();
    config.sync_active_profile();
    config.sources.push(SourceProfile::Bilibili {
        id: "first".into(),
        name: String::new(),
        account_id: None,
        credential_key: None,
        proxy: None,
    });
    config.sync_active_profile();
    assert_eq!(
        config.route_for(&bilibili("first")),
        Ok(NetworkRoute::System)
    );
}

#[test]
fn entries_saved_with_only_a_proxy_id_still_select_that_proxy() {
    let mut config = legacy();
    config.sync_network();
    let mut value = serde_json::to_value(&config).unwrap();
    let source = serde_json::to_value(bilibili("direct")).unwrap();
    let proxy = config.network.proxies[0].id.clone();
    value["network"]["source_proxies"] = serde_json::json!([
        { "source": source, "proxy_id": proxy }
    ]);
    let restored: Config = serde_json::from_value(value).unwrap();
    assert_eq!(
        restored.source_connection(&bilibili("direct")),
        SourceConnection::Proxy(proxy)
    );
}

#[test]
fn every_source_can_choose_system_direct_or_a_shared_proxy() {
    let mut config = legacy();
    config.sync_network();
    let subsonic = subsonic(&config);
    let bilibili = bilibili("direct");
    let proxy = config
        .store_proxy(None, "Local", " 127.0.0.1:7900 ")
        .unwrap();
    let profiles = config.sources.clone();
    for source in [&subsonic, &bilibili] {
        config
            .set_source_connection(source, SourceConnection::Proxy(proxy.clone()))
            .unwrap();
    }
    config
        .store_proxy(Some(&proxy), "Updated", "socks5h://127.0.0.1:7901")
        .unwrap();
    for source in [&subsonic, &bilibili] {
        assert_eq!(
            config.route_for(source),
            Ok(NetworkRoute::Proxy("socks5h://127.0.0.1:7901".into()))
        );
    }
    assert_eq!(config.sources, profiles);
    assert_eq!(config.proxy_users(&proxy), 2);
    assert_eq!(config.remove_proxy(&proxy), Err(NetworkError::ProxyInUse));
    config
        .set_source_connection(&bilibili, SourceConnection::Direct)
        .unwrap();
    config
        .set_source_connection(&subsonic, SourceConnection::System)
        .unwrap();
    assert_eq!(config.route_for(&bilibili), Ok(NetworkRoute::Direct));
    assert_eq!(config.route_for(&subsonic), Ok(NetworkRoute::System));
    assert_eq!(config.proxy_users(&proxy), 0);
    config.remove_proxy(&proxy).unwrap();
}

#[test]
fn removing_a_source_discards_only_its_route() {
    let mut config = legacy();
    config.sync_active_profile();
    let source = bilibili("a");
    config.sources.retain(|profile| profile.id() != source);
    config.sync_active_profile();
    assert_eq!(config.network.proxies.len(), 2);
    assert!(config
        .network
        .source_proxies
        .iter()
        .all(|entry| entry.source != source));
    assert_eq!(
        config.route_for(&bilibili("b")),
        Ok(NetworkRoute::Proxy("http://127.0.0.1:7890".into()))
    );
}

#[test]
fn proxy_addresses_are_normalized_and_may_use_socks() {
    for (input, expected) in [
        ("127.0.0.1:7890", "http://127.0.0.1:7890"),
        (" localhost:7890 ", "http://localhost:7890"),
        ("http://127.0.0.1:7890/", "http://127.0.0.1:7890"),
        ("https://proxy.example:8443", "https://proxy.example:8443"),
        ("socks5://127.0.0.1:1080", "socks5://127.0.0.1:1080"),
        ("SOCKS5H://127.0.0.1:1080", "socks5h://127.0.0.1:1080"),
    ] {
        assert_eq!(normalize_proxy(input).as_deref(), Ok(expected), "{input}");
    }
}

#[test]
fn invalid_proxy_and_missing_references_do_not_mutate_network_settings() {
    let mut config = legacy();
    config.sync_network();
    let before = config.network.clone();
    for input in [
        "",
        "   ",
        "file:///tmp/proxy",
        "ftp://127.0.0.1:21",
        "http://user:secret@localhost:7890",
        "socks5://user:secret@localhost:1080",
        "http://localhost:7890/path",
        "http://localhost:7890?token=secret",
        "http://localhost:7890#fragment",
    ] {
        assert_eq!(
            config.store_proxy(None, "", input),
            Err(NetworkError::InvalidProxy),
            "{input}"
        );
    }
    let source = bilibili("a");
    assert_eq!(
        config.set_source_connection(&source, SourceConnection::Proxy("missing".into())),
        Err(NetworkError::MissingProxy)
    );
    assert_eq!(
        config.set_source_connection(&bilibili("missing"), SourceConnection::Direct),
        Err(NetworkError::MissingSource)
    );
    assert_eq!(config.network, before);
    config.network.proxies.clear();
    assert_eq!(config.route_for(&source), Err(NetworkError::MissingProxy));
}

#[test]
fn released_files_without_network_settings_migrate_on_load() {
    let mut value = serde_json::to_value(legacy()).unwrap();
    value.as_object_mut().unwrap().remove("network");
    let mut restored: Config = serde_json::from_value(value).unwrap();
    restored.sync_active_profile();
    assert_eq!(
        restored.route_for(&bilibili("direct")),
        Ok(NetworkRoute::Direct)
    );
    assert_eq!(
        restored.route_for(&subsonic(&restored)),
        Ok(NetworkRoute::System)
    );
}

#[test]
fn lyrics_routes_are_independent_and_prevent_removing_their_saved_proxy() {
    let mut config = legacy();
    config.sync_network();
    let source = subsonic(&config);
    let proxy = config
        .store_proxy(None, "Lyrics", "http://127.0.0.1:7900")
        .unwrap();
    config.lyrics.connection = SourceConnection::Proxy(proxy.clone());
    assert_eq!(config.route_for(&source), Ok(NetworkRoute::System));
    assert_eq!(
        config.resolve_connection(&config.lyrics.connection),
        Ok(NetworkRoute::Proxy("http://127.0.0.1:7900".into()))
    );
    assert_eq!(config.proxy_users(&proxy), 1);
    assert_eq!(config.remove_proxy(&proxy), Err(NetworkError::ProxyInUse));
    let restored: Config = serde_json::from_value(serde_json::to_value(&config).unwrap()).unwrap();
    assert_eq!(restored.lyrics.connection, config.lyrics.connection);
    config.lyrics.connection = SourceConnection::Direct;
    config.remove_proxy(&proxy).unwrap();
    assert_eq!(config.route_for(&source), Ok(NetworkRoute::System));
}
