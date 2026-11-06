use super::*;
use crate::app::AppState;
use crate::models::{CapabilitySupport, OpenSubsonicExtension};

fn ping_info(name: &str) -> ServerInfo {
    let mut info = ServerInfo {
        app_name: Some(name.into()),
        server_version: Some("0.60.0".into()),
        protocol_version: "1.16.1".into(),
        ..Default::default()
    };
    info.refresh_capabilities();
    info
}

fn discovery_info(extensions: &[(&str, u32)]) -> ServerInfo {
    let mut info = ServerInfo {
        protocol_version: "unknown".into(),
        open_subsonic: true,
        extensions_discovered: true,
        extensions: extensions
            .iter()
            .map(|(name, version)| OpenSubsonicExtension {
                name: (*name).into(),
                versions: vec![*version],
            })
            .collect(),
        ..Default::default()
    };
    info.refresh_capabilities();
    info
}

fn connect(
    connection: &mut ServerConnection,
    requests: &mut LibraryRequests,
    name: &str,
) -> (LibraryRequest, LibraryRequest) {
    let ping = connection.begin(requests);
    let discovery = connection
        .complete_ping(ping, Ok(ping_info(name)), requests)
        .unwrap()
        .unwrap();
    (ping, discovery)
}

#[test]
fn successful_handshake_makes_server_usable_before_separate_discovery_completes() {
    let mut connection = ServerConnection::default();
    let mut requests = LibraryRequests::default();
    let ping = connection.begin(&mut requests);
    assert!(connection.loading());
    assert!(connection.server().is_none());
    assert_eq!(
        connection.complete_discovery(ping, Ok(discovery_info(&[]))),
        None
    );
    let discovery = connection
        .complete_ping(ping, Ok(ping_info("Navidrome")), &mut requests)
        .unwrap()
        .unwrap();
    assert_ne!(ping, discovery);
    assert!(!connection.loading());
    assert_eq!(
        connection.server().unwrap().app_name.as_deref(),
        Some("Navidrome")
    );
    assert_eq!(
        connection.server().unwrap().capabilities.structured_lyrics,
        CapabilitySupport::Unknown
    );
    assert!(connection.discovery.loading());
    assert_eq!(
        connection.complete_discovery(ping, Err("wrong stage".into())),
        None
    );
    assert_eq!(
        connection.complete_discovery(discovery, Ok(discovery_info(&[("songLyrics", 2)]))),
        Some(Ok(()))
    );
    assert_eq!(
        connection.server().unwrap().capabilities.enhanced_lyrics,
        CapabilitySupport::Supported
    );
}

#[test]
fn stale_and_duplicate_ping_results_cannot_replace_identity_or_restart_probe() {
    let mut connection = ServerConnection::default();
    let mut requests = LibraryRequests::default();
    let old = connection.begin(&mut requests);
    let current = connection.begin(&mut requests);
    for result in [Ok(ping_info("old")), Err("old ping error".into())] {
        assert!(connection
            .complete_ping(old, result, &mut requests)
            .is_none());
        assert!(connection.loading());
        assert!(connection.server().is_none());
    }
    let probe = connection
        .complete_ping(current, Ok(ping_info("current")), &mut requests)
        .unwrap()
        .unwrap();
    for result in [
        Ok(ping_info("duplicate")),
        Err("duplicate ping error".into()),
    ] {
        assert!(connection
            .complete_ping(current, result, &mut requests)
            .is_none());
        assert!(!connection.loading());
        assert_eq!(
            connection.server().unwrap().app_name.as_deref(),
            Some("current")
        );
        assert!(connection.discovery.loading());
    }
    assert_eq!(
        connection.complete_discovery(probe, Ok(discovery_info(&[("sonicSimilarity", 1)]))),
        Some(Ok(()))
    );
    assert_eq!(
        connection.server().unwrap().capabilities.sonic_similarity,
        CapabilitySupport::Supported
    );
}

#[test]
fn duplicate_probe_results_cannot_downgrade_capabilities_or_replace_metadata() {
    let mut connection = ServerConnection::default();
    let mut requests = LibraryRequests::default();
    let (_, probe) = connect(&mut connection, &mut requests, "Navidrome");
    assert_eq!(
        connection.complete_discovery(
            probe,
            Ok(discovery_info(&[("songLyrics", 2), ("sonicSimilarity", 1)]))
        ),
        Some(Ok(()))
    );
    let discovered = connection.server().unwrap().clone();
    let mut duplicate = discovery_info(&[]);
    duplicate.app_name = Some("replacement".into());
    for result in [Ok(duplicate), Err("duplicate failure".into())] {
        assert_eq!(connection.complete_discovery(probe, result), None);
        assert_eq!(connection.server(), Some(&discovered));
        assert!(connection.discovery.error().is_none());
    }
}

#[test]
fn optional_probe_failure_preserves_compatibility_and_is_consumed_once() {
    let mut connection = ServerConnection::default();
    let mut requests = LibraryRequests::default();
    let (_, probe) = connect(&mut connection, &mut requests, "legacy");
    let connected = connection.server().unwrap().clone();
    assert_eq!(
        connection.complete_discovery(probe, Err("endpoint unavailable".into())),
        Some(Err("endpoint unavailable"))
    );
    assert!(!connection.loading());
    assert_eq!(connection.server(), Some(&connected));
    let capabilities = connection.server().unwrap().capabilities;
    assert!(capabilities.structured_lyrics.allows_optimistic_request());
    assert!(capabilities.sonic_similarity.allows_optimistic_request());
    assert!(!connection.server().unwrap().extensions_discovered);
    for result in [
        Ok(discovery_info(&[("songLyrics", 2)])),
        Err("duplicate failure".into()),
    ] {
        assert_eq!(connection.complete_discovery(probe, result), None);
        assert_eq!(connection.discovery.error(), Some("endpoint unavailable"));
        assert_eq!(connection.server(), Some(&connected));
    }
    let (_, retry) = connect(&mut connection, &mut requests, "legacy");
    assert!(connection.discovery.error().is_none());
    assert_eq!(
        connection.complete_discovery(probe, Ok(discovery_info(&[]))),
        None
    );
    assert_eq!(
        connection.complete_discovery(retry, Ok(discovery_info(&[("songLyrics", 1)]))),
        Some(Ok(()))
    );
    assert_eq!(
        connection.server().unwrap().capabilities.structured_lyrics,
        CapabilitySupport::Supported
    );
}

#[test]
fn handshake_failure_preserves_previous_identity_and_never_admits_old_probe() {
    let mut requests = LibraryRequests::default();
    for previously_connected in [false, true] {
        let mut connection = ServerConnection::default();
        let old_probe =
            previously_connected.then(|| connect(&mut connection, &mut requests, "previous").1);
        let previous = connection.server().cloned();
        let current = connection.begin(&mut requests);
        if let Some(old_probe) = old_probe {
            // Invalidation happens when refreshing starts, before its result arrives.
            assert_eq!(
                connection.complete_discovery(old_probe, Ok(discovery_info(&[("songLyrics", 2)]))),
                None
            );
        }
        assert_eq!(
            connection.complete_ping(current, Err("offline".into()), &mut requests),
            Some(Err("offline"))
        );
        assert_eq!(connection.server(), previous.as_ref());
        assert!(!connection.loading());
        assert_eq!(
            connection.complete_discovery(current, Ok(discovery_info(&[]))),
            None
        );
        assert!(connection
            .complete_ping(current, Ok(ping_info("duplicate")), &mut requests)
            .is_none());
        assert_eq!(connection.ping.error(), Some("offline"));
        if let Some(old_probe) = old_probe {
            assert_eq!(
                connection.complete_discovery(old_probe, Err("old probe error".into())),
                None
            );
        }
        let retry = connection.begin(&mut requests);
        assert!(connection.ping.error().is_none());
        assert!(connection
            .complete_ping(current, Err("old error".into()), &mut requests)
            .is_none());
        assert!(connection
            .complete_ping(retry, Ok(ping_info("retried")), &mut requests)
            .unwrap()
            .is_ok());
        assert_eq!(
            connection.server().unwrap().app_name.as_deref(),
            Some("retried")
        );
    }
}

#[test]
fn successful_refresh_resets_old_capabilities_and_rejects_prior_discovery() {
    let mut connection = ServerConnection::default();
    let mut requests = LibraryRequests::default();
    let (_, old) = connect(&mut connection, &mut requests, "first");
    connection.complete_discovery(old, Ok(discovery_info(&[("songLyrics", 2)])));
    let ping = connection.begin(&mut requests);
    assert!(connection
        .server()
        .unwrap()
        .capabilities
        .enhanced_lyrics
        .is_supported());
    let current = connection
        .complete_ping(ping, Ok(ping_info("updated")), &mut requests)
        .unwrap()
        .unwrap();
    assert_eq!(
        connection.server().unwrap().capabilities.enhanced_lyrics,
        CapabilitySupport::Unknown
    );
    assert_eq!(
        connection.server().unwrap().app_name.as_deref(),
        Some("updated")
    );
    assert_eq!(
        connection.complete_discovery(old, Ok(discovery_info(&[("songLyrics", 2)]))),
        None
    );
    assert_eq!(
        connection.complete_discovery(current, Ok(discovery_info(&[]))),
        Some(Ok(()))
    );
    assert_eq!(
        connection.server().unwrap().capabilities.enhanced_lyrics,
        CapabilitySupport::Unsupported
    );
}

#[test]
fn discovery_merges_optional_identity_fields_and_refreshes_protocol_capabilities() {
    let mut connection = ServerConnection::default();
    let mut requests = LibraryRequests::default();
    let (_, probe) = connect(&mut connection, &mut requests, "Navidrome");
    assert_eq!(
        connection.complete_discovery(probe, Ok(discovery_info(&[("topSongsByArtistId", 1)]))),
        Some(Ok(()))
    );
    let server = connection.server().unwrap();
    assert_eq!(server.app_name.as_deref(), Some("Navidrome"));
    assert_eq!(server.display_version(), "0.60.0");
    assert_eq!(server.protocol_version, "1.16.1");
    assert_eq!(
        server.capabilities.internet_radio,
        CapabilitySupport::Supported
    );
    assert_eq!(
        server.capabilities.top_songs_by_artist_id,
        CapabilitySupport::Supported
    );
    assert_eq!(
        server.capabilities.structured_lyrics,
        CapabilitySupport::Unsupported
    );

    let (_, probe) = connect(&mut connection, &mut requests, "Navidrome");
    let mut discovered = discovery_info(&[("songLyrics", 1)]);
    discovered.app_name = Some("gonic".into());
    discovered.server_version = Some("0.18.0".into());
    discovered.protocol_version = "1.8.0".into();
    assert_eq!(
        connection.complete_discovery(probe, Ok(discovered)),
        Some(Ok(()))
    );
    let server = connection.server().unwrap();
    assert_eq!(server.app_name.as_deref(), Some("gonic"));
    assert_eq!(server.display_version(), "0.18.0");
    assert_eq!(
        server.capabilities.internet_radio,
        CapabilitySupport::Unsupported
    );
    assert_eq!(server.capabilities.podcasts, CapabilitySupport::Supported);
}

#[test]
fn server_reset_rejects_old_handshake_and_probe_responses_without_consuming_new_requests() {
    let mut state = AppState::default();
    let mut requests = LibraryRequests::default();
    let (old_ping, old_probe) = connect(&mut state.connection, &mut requests, "old server");
    state = AppState::default();
    assert!(state.connection.server().is_none());
    let ping = state.connection.begin(&mut requests);
    assert_ne!(old_ping, ping);
    for old in [old_ping, old_probe] {
        assert!(state
            .connection
            .complete_ping(old, Ok(ping_info("old")), &mut requests)
            .is_none());
        assert!(state
            .connection
            .complete_ping(old, Err("old error".into()), &mut requests)
            .is_none());
        assert_eq!(
            state
                .connection
                .complete_discovery(old, Ok(discovery_info(&[]))),
            None
        );
        assert_eq!(
            state
                .connection
                .complete_discovery(old, Err("old probe error".into())),
            None
        );
        assert!(state.connection.loading());
        assert!(state.connection.server().is_none());
    }
    let probe = state
        .connection
        .complete_ping(ping, Ok(ping_info("current")), &mut requests)
        .unwrap()
        .unwrap();
    assert_ne!(old_probe, probe);
    assert_eq!(
        state
            .connection
            .complete_discovery(old_probe, Ok(discovery_info(&[]))),
        None
    );
    assert_eq!(
        state
            .connection
            .complete_discovery(probe, Ok(discovery_info(&[("songLyrics", 2)]))),
        Some(Ok(()))
    );
    assert_eq!(
        state.connection.server().unwrap().app_name.as_deref(),
        Some("current")
    );
    assert_eq!(
        state
            .connection
            .server()
            .unwrap()
            .capabilities
            .enhanced_lyrics,
        CapabilitySupport::Supported
    );
}
