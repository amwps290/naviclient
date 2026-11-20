use super::*;
use crate::playlist::loading::{playlist_update_channel, PlaylistUpdate};
use crate::test_support::http::{success, Server};
use serde_json::json;

#[tokio::test]
async fn opensubsonic_playlist_loading_uses_bounded_ordered_batches() {
    let entries: Vec<_> = (0..300)
        .map(|index| json!({"id":index.to_string(),"title":format!("Track {index}")}))
        .collect();
    let server = Server::new(vec![success(json!({"subsonic-response": {
        "status": "ok",
        "playlist": {
            "id": "large",
            "name": "Large playlist",
            "songCount": 300,
            "entry": entries
        }
    }}))]);
    let source =
        MusicSource::opensubsonic("stable-profile", &server.url(""), "fixture", "password")
            .unwrap();
    let expected_source = source.id().clone();
    let (sender, mut receiver) = playlist_update_channel();
    let load = tokio::spawn(async move { source.load_playlist("large", sender).await });

    let mut batch_sizes = Vec::new();
    let mut final_count = None;
    while let Some(update) = receiver.recv().await {
        match update {
            PlaylistUpdate::Metadata(playlist) => assert_eq!(playlist.id, "large"),
            PlaylistUpdate::Batch { songs, progress } => {
                assert!(songs
                    .iter()
                    .all(|song| song.source.as_ref() == Some(&expected_source)));
                batch_sizes.push(songs.len());
                assert_eq!(progress.songs, batch_sizes.iter().sum::<usize>());
            }
            PlaylistUpdate::Complete(progress) => {
                final_count = Some(progress.songs);
                break;
            }
            PlaylistUpdate::Progress(_) => {}
            PlaylistUpdate::Failed(error) => panic!("playlist load failed: {}", error.message),
        }
    }
    load.await.unwrap();

    assert_eq!(batch_sizes, [128, 128, 44]);
    assert_eq!(final_count, Some(300));
    assert_eq!(server.requests()[0].url.path(), "/rest/getPlaylist.view");
}

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
