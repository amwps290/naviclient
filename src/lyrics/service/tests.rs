use super::*;
use crate::source::{ProviderKind, SourceId};
use crate::test_support::http::{success, Server};
use serde_json::json;

fn song() -> Song {
    Song {
        id: "song".into(),
        title: "Test".into(),
        artist: "Artist".into(),
        duration: Some(223),
        source: Some(SourceId::new(ProviderKind::OpenSubsonic, "fixture")),
        ..Default::default()
    }
}
fn record() -> serde_json::Value {
    json!({"id":7,"trackName":"Test","artistName":"Artist","albumName":"","duration":223,"instrumental":false,"syncedLyrics":"[00:01]test","plainLyrics":"test"})
}
fn service(
    server: &Server,
    cache: LyricsCache,
    policy: LyricsPolicy,
    enabled: bool,
) -> LyricsService {
    LyricsService {
        settings: LyricsSettings {
            enabled,
            policy,
            ..Default::default()
        },
        provider: Ok(Lrclib::test_client(server.url(""))),
        cache,
    }
}
fn cache(dir: &tempfile::TempDir) -> LyricsCache {
    LyricsCache::at(dir.path().join("cache.json"))
}

fn native_source(server: &Server) -> MusicSource {
    MusicSource::opensubsonic_with_route(
        "fixture",
        &server.url(""),
        "user",
        "password",
        crate::models::network::NetworkRoute::Direct,
    )
    .unwrap()
}

#[tokio::test]
async fn native_http_failure_stays_an_error_when_online_is_disabled_or_manual() {
    let dir = tempfile::tempdir().unwrap();
    let online = Server::new(vec![]);
    let native = Server::with_handler(|_| (500, "failed".into()));
    let source = native_source(&native);
    let song = song();
    for (policy, enabled) in [
        (LyricsPolicy::SourceFirst, false),
        (LyricsPolicy::OnlineFirst, false),
        (LyricsPolicy::Manual, true),
    ] {
        let outcome = service(&online, cache(&dir), policy, enabled)
            .load(
                &song,
                &song.media_key().unwrap(),
                Some(&source),
                CapabilitySupport::Unsupported,
                false,
                |_, _| {},
            )
            .await;
        assert_eq!(outcome.status, LyricsStatus::SourceUnavailable);
        assert!(outcome.error.unwrap().contains("500"));
        assert!(outcome.document.is_none());
    }
    assert!(online.requests().is_empty());
}

#[tokio::test]
async fn native_empty_success_is_distinct_from_failure() {
    let dir = tempfile::tempdir().unwrap();
    let online = Server::new(vec![]);
    let native = Server::with_handler(|_| success(json!({"subsonic-response":{"status":"ok"}})));
    let source = native_source(&native);
    let song = song();
    let outcome = service(&online, cache(&dir), LyricsPolicy::Manual, true)
        .load(
            &song,
            &song.media_key().unwrap(),
            Some(&source),
            CapabilitySupport::Unsupported,
            false,
            |_, _| {},
        )
        .await;
    assert_eq!(outcome.status, LyricsStatus::Empty);
    assert!(outcome.error.is_none());
    assert!(outcome.document.is_none());
}

#[tokio::test]
async fn native_failure_can_fall_back_to_online_but_is_preserved_if_no_match() {
    let dir = tempfile::tempdir().unwrap();
    let native = Server::with_handler(|_| (500, "failed".into()));
    let source = native_source(&native);
    let song = song();
    let online = Server::new(vec![
        success(record()),
        (404, "missing".into()),
        success(json!([])),
    ]);
    let service = service(&online, cache(&dir), LyricsPolicy::SourceFirst, true);
    let outcome = service
        .load(
            &song,
            &song.media_key().unwrap(),
            Some(&source),
            CapabilitySupport::Unsupported,
            true,
            |_, _| {},
        )
        .await;
    assert_eq!(outcome.status, LyricsStatus::Ready);
    assert!(outcome.error.is_none());
    assert_eq!(
        outcome.document.unwrap().origin,
        crate::lyrics::LyricsOrigin::Lrclib
    );
    let outcome = service
        .load(
            &song,
            &song.media_key().unwrap(),
            Some(&source),
            CapabilitySupport::Unsupported,
            true,
            |_, _| {},
        )
        .await;
    assert_eq!(outcome.status, LyricsStatus::SourceUnavailable);
    assert_eq!(outcome.warning, Some(LyricsStatus::NoMatch));
    assert!(outcome.error.unwrap().contains("500"));
}

#[tokio::test]
async fn online_first_preserves_both_failures_if_native_fallback_also_fails() {
    let dir = tempfile::tempdir().unwrap();
    let native = Server::new(vec![(500, "failed".into())]);
    let source = native_source(&native);
    let song = song();
    let online = Server::new(vec![(500, "unavailable".into())]);
    let outcome = service(&online, cache(&dir), LyricsPolicy::OnlineFirst, true)
        .load(
            &song,
            &song.media_key().unwrap(),
            Some(&source),
            CapabilitySupport::Unsupported,
            false,
            |_, _| {},
        )
        .await;
    assert_eq!(outcome.status, LyricsStatus::SourceUnavailable);
    assert_eq!(outcome.warning, Some(LyricsStatus::Unavailable));
    assert!(outcome.error.unwrap().contains("500"));
}

#[tokio::test(start_paused = true)]
async fn native_deadline_preserves_timeout_instead_of_returning_empty() {
    let outcome = native_lyrics(std::future::pending())
        .await
        .err()
        .unwrap()
        .outcome();
    assert_eq!(outcome.status, LyricsStatus::SourceTimedOut);
    assert!(outcome.document.is_none());
    assert!(outcome.error.unwrap().contains("timed out"));
}

#[tokio::test]
async fn stalled_native_http_requests_preserve_timeout_for_disabled_and_manual_policies() {
    use tokio::io::AsyncReadExt;
    let dir = tempfile::tempdir().unwrap();
    let online = Server::new(vec![]);
    for (policy, enabled) in [
        (LyricsPolicy::SourceFirst, false),
        (LyricsPolicy::OnlineFirst, false),
        (LyricsPolicy::Manual, true),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (received_tx, received_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut header = [0; 1024];
            assert!(stream.read(&mut header).await.unwrap() > 0);
            received_tx.send(()).unwrap();
            std::future::pending::<()>().await;
            drop(stream);
        });
        let source = MusicSource::opensubsonic_with_route(
            "fixture",
            &format!("http://{address}"),
            "user",
            "password",
            crate::models::network::NetworkRoute::Direct,
        )
        .unwrap();
        let service = service(&online, cache(&dir), policy, enabled);
        let load = tokio::spawn(async move {
            let song = song();
            service
                .load(
                    &song,
                    &song.media_key().unwrap(),
                    Some(&source),
                    CapabilitySupport::Unsupported,
                    false,
                    |_, _| {},
                )
                .await
        });
        // Pause only after real HTTP has reached the server, so the virtual
        // clock cannot expire the request before socket I/O gets scheduled.
        received_rx.await.unwrap();
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(6)).await;
        let outcome = load.await.unwrap();
        tokio::time::resume();
        server.abort();
        assert_eq!(outcome.status, LyricsStatus::SourceTimedOut);
        assert!(outcome.error.unwrap().contains("timed out"));
        assert!(outcome.document.is_none());
    }
    assert!(online.requests().is_empty());
}

#[tokio::test]
async fn disabled_and_manual_policies_never_access_online_provider() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::new(vec![]);
    let song = song();
    for (policy, enabled) in [
        (LyricsPolicy::SourceFirst, false),
        (LyricsPolicy::OnlineFirst, false),
        (LyricsPolicy::Manual, true),
    ] {
        let outcome = service(&server, cache(&dir), policy, enabled)
            .load(
                &song,
                &song.media_key().unwrap(),
                None,
                CapabilitySupport::Unsupported,
                false,
                |_, _| {},
            )
            .await;
        assert_eq!(outcome.status, LyricsStatus::Empty);
    }
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn original_lyrics_win_without_online_requests_and_online_failure_falls_back() {
    let dir = tempfile::tempdir().unwrap();
    let online = Server::new(vec![(500, "offline".into())]);
    let native = Server::with_handler(|_| {
        success(json!({"subsonic-response":{"status":"ok","lyrics":{"value":"native text"}}}))
    });
    let source = MusicSource::opensubsonic_with_route(
        "fixture",
        &native.url(""),
        "user",
        "password",
        crate::models::network::NetworkRoute::Direct,
    )
    .unwrap();
    let song = song();
    let outcome = service(&online, cache(&dir), LyricsPolicy::SourceFirst, true)
        .load(
            &song,
            &song.media_key().unwrap(),
            Some(&source),
            CapabilitySupport::Unsupported,
            false,
            |_, _| {},
        )
        .await;
    assert_eq!(
        outcome.document.unwrap().lyrics.lines[0].text,
        "native text"
    );
    assert!(online.requests().is_empty());
    let outcome = service(&online, cache(&dir), LyricsPolicy::OnlineFirst, true)
        .load(
            &song,
            &song.media_key().unwrap(),
            Some(&source),
            CapabilitySupport::Unsupported,
            false,
            |_, _| {},
        )
        .await;
    assert_eq!(
        outcome.document.unwrap().lyrics.lines[0].text,
        "native text"
    );
    assert_eq!(outcome.warning, Some(LyricsStatus::Unavailable));
}

#[tokio::test]
async fn missing_original_lyrics_get_online_result_and_cached_replay() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::new(vec![success(record())]);
    let song = song();
    let cache = cache(&dir);
    let service = service(&server, cache.clone(), LyricsPolicy::SourceFirst, true);
    for _ in 0..2 {
        let outcome = service
            .load(
                &song,
                &song.media_key().unwrap(),
                None,
                CapabilitySupport::Unsupported,
                false,
                |_, _| {},
            )
            .await;
        assert_eq!(outcome.status, LyricsStatus::Ready);
        assert_eq!(
            outcome.document.unwrap().origin,
            crate::lyrics::LyricsOrigin::Lrclib
        );
    }
    assert_eq!(server.requests().len(), 1);
}

#[tokio::test]
async fn bilibili_uploader_and_unknown_duration_are_never_auto_matched() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::new(vec![]);
    for song in [
        Song {
            source: Some(SourceId::new(ProviderKind::Bilibili, "fixture")),
            ..song()
        },
        Song {
            duration: None,
            ..song()
        },
    ] {
        let outcome = service(&server, cache(&dir), LyricsPolicy::SourceFirst, true)
            .load(
                &song,
                &song.media_key().unwrap(),
                None,
                CapabilitySupport::Unsupported,
                false,
                |_, _| {},
            )
            .await;
        assert_eq!(outcome.status, LyricsStatus::MetadataNeeded);
    }
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn pins_work_offline_and_are_not_overridden_by_policies() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::new(vec![]);
    let cache = cache(&dir);
    let song = song();
    let mut document = Document::online(serde_json::from_value(record()).unwrap());
    document.offset_ms = 500;
    cache
        .remember(&song.media_key().unwrap(), &document)
        .unwrap();
    let outcome = service(&server, cache, LyricsPolicy::OnlineFirst, false)
        .load(
            &song,
            &song.media_key().unwrap(),
            None,
            CapabilitySupport::Unsupported,
            true,
            |_, _| {},
        )
        .await;
    assert!(outcome.document.as_ref().unwrap().manual);
    assert_eq!(outcome.document.unwrap().offset_ms, 500);
    assert!(server.requests().is_empty());
}

#[tokio::test]
async fn http_200_metadata_is_verified_and_complete_misses_are_negative_cached() {
    let dir = tempfile::tempdir().unwrap();
    let mut wrong = record();
    wrong["artistName"] = json!("Wrong");
    let server = Server::new(vec![success(wrong), success(json!([]))]);
    let song = song();
    let cache = cache(&dir);
    let service = service(&server, cache.clone(), LyricsPolicy::SourceFirst, true);
    let outcome = service
        .load(
            &song,
            &song.media_key().unwrap(),
            None,
            CapabilitySupport::Unsupported,
            false,
            |_, _| {},
        )
        .await;
    assert_eq!(outcome.status, LyricsStatus::NoMatch);
    assert_eq!(cache.automatic(&Query::for_song(&song)), Some(None));
}

#[tokio::test]
async fn transport_errors_do_not_create_negative_cache_entries() {
    let dir = tempfile::tempdir().unwrap();
    let server = Server::new(vec![(500, "bad gateway".into())]);
    let song = song();
    let cache = cache(&dir);
    let outcome = service(&server, cache.clone(), LyricsPolicy::SourceFirst, true)
        .load(
            &song,
            &song.media_key().unwrap(),
            None,
            CapabilitySupport::Unsupported,
            false,
            |_, _| {},
        )
        .await;
    assert_eq!(outcome.status, LyricsStatus::Unavailable);
    assert!(cache.automatic(&Query::for_song(&song)).is_none());
}

#[tokio::test]
async fn restoring_a_selection_resets_preferences_and_loads_the_native_lyrics() {
    let directory = tempfile::tempdir().unwrap();
    let cache = cache(&directory);
    let song = song();
    let media = song.media_key().unwrap();
    let mut selected = Document::online(serde_json::from_value(record()).unwrap());
    selected.offset_ms = 750;
    selected.plain = true;
    selected.query = Some(Query::for_song(&song));
    cache.remember(&media, &selected).unwrap();
    cache.restore(&media).unwrap();
    let online = Server::new(vec![]);
    let native = Server::new(vec![success(
        json!({"subsonic-response":{"status":"ok","lyrics":{"value":"native text"}}}),
    )]);
    let source = MusicSource::opensubsonic_with_route(
        "fixture",
        &native.url(""),
        "user",
        "password",
        crate::models::network::NetworkRoute::Direct,
    )
    .unwrap();
    let outcome = service(&online, cache.clone(), LyricsPolicy::SourceFirst, true)
        .load(
            &song,
            &media,
            Some(&source),
            CapabilitySupport::Unsupported,
            true,
            |_, _| {},
        )
        .await;
    let document = outcome.document.unwrap();
    assert_eq!(document.lyrics.lines[0].text, "native text");
    assert!(!document.manual);
    assert!(!document.plain);
    assert_eq!(document.offset_ms, 0);
    assert!(document.query.is_none());
    assert!(cache.selected(&media).is_none());
    assert!(online.requests().is_empty());
}

#[tokio::test]
async fn unusable_remembered_body_is_recovered_by_id_with_its_preferences() {
    let directory = tempfile::tempdir().unwrap();
    let cache = cache(&directory);
    let song = song();
    let media = song.media_key().unwrap();
    let mut selected = Document::online(super::super::provider::Record {
        id: 7,
        ..Default::default()
    });
    selected.offset_ms = -500;
    cache.remember(&media, &selected).unwrap();
    let online = Server::new(vec![success(record())]);
    let outcome = service(&online, cache.clone(), LyricsPolicy::SourceFirst, true)
        .load(
            &song,
            &media,
            None,
            CapabilitySupport::Unsupported,
            false,
            |_, _| {},
        )
        .await;
    let document = outcome.document.unwrap();
    assert!(document.manual);
    assert_eq!(document.offset_ms, -500);
    assert_eq!(cache.selected(&media).unwrap().lyrics.lines[0].text, "test");
    assert_eq!(online.requests()[0].url.path(), "/api/get/7");
}
