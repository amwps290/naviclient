use super::*;

#[test]
fn saving_a_proxy_and_source_selection_survives_reload_without_changing_credentials() {
    let mut config = Config::default();
    config.sync_active_profile();
    let source = config.sources[0].id();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("config.json");
    let next = save_network_change(
        &config,
        |config| {
            let proxy = config.store_proxy(None, "Local", "http://127.0.0.1:7890")?;
            config.set_source_connection(
                &source,
                crate::models::network::SourceConnection::Proxy(proxy),
            )
        },
        |config| {
            std::fs::write(&path, serde_json::to_vec(config)?)?;
            Ok(())
        },
    )
    .unwrap();
    let restored: Config = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(
        restored.route_for(&source),
        Ok(crate::models::network::NetworkRoute::Proxy(
            "http://127.0.0.1:7890".into()
        ))
    );
    assert_eq!(restored.sources, config.sources);
    assert_eq!(restored.servers, config.servers);
    assert_eq!(restored.active_source, config.active_source);
    assert_eq!(
        serde_json::to_value(next).unwrap(),
        serde_json::to_value(restored).unwrap()
    );
}

#[test]
fn failed_persistence_keeps_the_original_settings_and_source_owners() {
    let config = Config::default();
    let before = serde_json::to_value(&config).unwrap();
    assert!(matches!(
        save_network_change(
            &config,
            |config| config
                .store_proxy(None, "Local", "http://127.0.0.1:7890")
                .map(|_| ()),
            |_| Err(anyhow::anyhow!("disk unavailable"))
        ),
        Err(SaveError::Persistence)
    ));
    assert_eq!(serde_json::to_value(&config).unwrap(), before);
}

#[test]
fn invalid_proxy_never_reaches_persistence() {
    assert!(matches!(
        save_network_change(
            &Config::default(),
            |config| config.store_proxy(None, "", "file:///proxy").map(|_| ()),
            |_| panic!("saved invalid settings")
        ),
        Err(SaveError::Network(NetworkError::InvalidProxy))
    ));
}
