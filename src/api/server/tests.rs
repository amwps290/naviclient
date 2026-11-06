use super::*;
use crate::models::CapabilitySupport;

#[test]
fn parses_server_identity_and_open_subsonic_extensions() {
    let ping = serde_json::json!({
        "status": "ok",
        "version": "1.16.1",
        "type": "ExampleSonic",
        "serverVersion": "2.4.0",
        "openSubsonic": true
    });
    let mut info = parse_server_info(&ping);
    assert_eq!(
        info.capabilities.structured_lyrics,
        CapabilitySupport::Unknown
    );
    let capabilities = serde_json::json!({
        "status": "ok",
        "version": "1.16.1",
        "type": "ExampleSonic",
        "serverVersion": "2.4.1",
        "openSubsonicExtensions": [
            {"name": "sonicSimilarity", "versions": [1]},
            {"name": "songLyrics", "versions": [2, 1]},
            {"name": "songLyrics", "versions": [2]},
            {"name": "", "versions": [1]},
            {"versions": [1]}
        ]
    });

    merge_open_subsonic_capabilities(&mut info, &capabilities);

    assert_eq!(info.app_name.as_deref(), Some("ExampleSonic"));
    assert_eq!(info.server_version.as_deref(), Some("2.4.1"));
    assert_eq!(info.protocol_version, "1.16.1");
    assert!(info.open_subsonic);
    assert!(info.extensions_discovered);
    assert!(info.supports_extension("sonicSimilarity", 1));
    assert!(info.supports_extension("SONGLYRICS", 2));
    assert!(!info.supports_extension("songLyrics", 3));
    assert_eq!(
        info.capabilities.structured_lyrics,
        CapabilitySupport::Supported
    );
    assert_eq!(
        info.capabilities.enhanced_lyrics,
        CapabilitySupport::Supported
    );
    assert_eq!(
        info.capabilities.sonic_similarity,
        CapabilitySupport::Supported
    );
    assert_eq!(
        info.capabilities.top_songs_by_artist_id,
        CapabilitySupport::Unsupported
    );
    assert_eq!(
        info.extension_summary(6).as_deref(),
        Some("songLyrics v1/2, sonicSimilarity v1")
    );

    let mut connected = parse_server_info(&serde_json::json!({
        "status": "ok",
        "version": "1.16.1"
    }));
    connected.merge_open_subsonic_info(info.clone());
    assert_eq!(connected, info);
}

#[test]
fn legacy_ping_defaults_to_subsonic_compatibility_mode() {
    let info = parse_server_info(&serde_json::json!({
        "status": "ok",
        "version": "1.13.0",
        "app": "Legacy Server"
    }));

    assert_eq!(info.app_name.as_deref(), Some("Legacy Server"));
    assert_eq!(info.display_version(), "1.13.0");
    assert!(!info.open_subsonic);
    assert!(!info.extensions_discovered);
    assert!(info.extensions.is_empty());
    assert_eq!(
        info.capabilities.structured_lyrics,
        CapabilitySupport::Unknown
    );
    assert_eq!(
        info.capabilities.internet_radio,
        CapabilitySupport::Supported
    );
    assert_eq!(info.extension_summary(4), None);
}
