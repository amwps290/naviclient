use super::*;
use crate::test_support::http::{success, Server};
use serde_json::json;

#[tokio::test]
async fn search_dispatch_preserves_remote_ids_counts_and_subsonic_authentication() {
    let server = Server::new(vec![success(json!({"subsonic-response": {
        "status": "ok", "searchResult3": {"song": [{"id":"remote-id", "title":"Title"}]}
    }}))]);
    let source =
        MusicSource::opensubsonic("stable-profile", &server.url(""), "fixture", "password")
            .unwrap();
    let found = source.search("music", 120, 80, 60).await.unwrap();
    assert_eq!(found.songs[0].id, "remote-id");
    assert_eq!(
        found.songs[0].media_key().unwrap().source,
        source.id().clone()
    );
    let requests = server.requests();
    assert_eq!(requests[0].url.path(), "/rest/search3.view");
    let params: std::collections::HashMap<_, _> = requests[0].url.query_pairs().collect();
    for (key, value) in [
        ("query", "music"),
        ("songCount", "120"),
        ("albumCount", "80"),
        ("artistCount", "60"),
        ("u", "fixture"),
    ] {
        assert_eq!(params.get(key).unwrap(), value);
    }
    assert!(params.contains_key("t"));
    assert!(!requests[0].url.as_str().contains("password"));
}

#[tokio::test]
async fn playback_rejects_a_foreign_media_identity_before_constructing_a_request() {
    let source =
        MusicSource::opensubsonic("profile-a", "http://127.0.0.1:1", "user", "password").unwrap();
    for owner in [
        SourceId::new(ProviderKind::OpenSubsonic, "profile-b"),
        SourceId::new(ProviderKind::Bilibili, "profile-a"),
    ] {
        let song = Song {
            source: Some(owner),
            id: "same-id".into(),
            ..Song::default()
        };
        let error = source
            .resolve_playback(&song, TranscodingQuality::Original)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("another source"));
    }
}

#[tokio::test]
async fn playback_resolution_uses_profile_identity_and_preserves_opus_fallback() {
    let source =
        MusicSource::opensubsonic("profile-a", "http://127.0.0.1:1", "user", "password").unwrap();
    let song = Song {
        id: "remote-id".into(),
        suffix: Some("opus".into()),
        duration: Some(180),
        ..Song::default()
    };
    let resolved = source
        .resolve_playback(&song, TranscodingQuality::Original)
        .await
        .unwrap();
    assert_eq!(resolved.media.source, source.id().clone());
    assert_eq!(resolved.media.remote_id, song.id);
    assert_eq!(resolved.duration, Some(std::time::Duration::from_secs(180)));
    assert_eq!(resolved.http.cache_policy(), CachePolicy::Persistent);
    assert!(resolved.http.length_is_estimate());
    let url = url::Url::parse(&resolved.http).unwrap();
    let params: std::collections::HashMap<_, _> = url.query_pairs().collect();
    assert_eq!(params.get("id").unwrap(), "remote-id");
    assert_eq!(params.get("format").unwrap(), "mp3");
    let other =
        MusicSource::opensubsonic("profile-b", "http://127.0.0.1:1", "user", "password").unwrap();
    assert_ne!(
        resolved.cache_key(),
        other
            .resolve_playback(&song, TranscodingQuality::Original)
            .await
            .unwrap()
            .cache_key()
    );
}
