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
        provider: Ok(vec![Provider::Lrclib(
            crate::lyrics::lrclib::Lrclib::test_client(server.url("")),
        )]),
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

fn kugou_record() -> serde_json::Value {
    json!({"id":"7","song":"Test","singer":"Artist","duration":223000,"accesskey":"temporary-key"})
}
fn kugou_lyrics() -> serde_json::Value {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    json!({"status":200,"id":"7","content":STANDARD.encode("[00:01]test")})
}
fn multi_service(
    lrclib: &Server,
    kugou: &Server,
    netease: &Server,
    cache: LyricsCache,
) -> LyricsService {
    LyricsService {
        settings: LyricsSettings {
            enabled: true,
            sources: ProviderId::ALL.to_vec(),
            ..Default::default()
        },
        provider: Ok(vec![
            Provider::Lrclib(crate::lyrics::lrclib::Lrclib::test_client(lrclib.url(""))),
            Provider::Kugou(crate::lyrics::kugou::Kugou::test_client(kugou.url(""))),
            Provider::Netease(crate::lyrics::netease::Netease::test_client(
                netease.url(""),
            )),
        ]),
        cache,
    }
}

#[tokio::test]
async fn lrclib_negative_cache_does_not_suppress_kugou_fallback_and_replay() {
    let dir = tempfile::tempdir().unwrap();
    let lrclib = Server::new(vec![]);
    let kugou = Server::new(vec![
        success(json!({"status":200,"candidates":[kugou_record()]})),
        success(kugou_lyrics()),
    ]);
    let netease = Server::new(vec![]);
    let song = song();
    let cache = cache(&dir);
    cache.save_automatic(&Query::for_song(&song), None).unwrap();
    let service = multi_service(&lrclib, &kugou, &netease, cache);
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
        assert_eq!(
            outcome.document.unwrap().origin,
            crate::lyrics::LyricsOrigin::Kugou
        );
    }
    assert!(lrclib.requests().is_empty());
    assert!(netease.requests().is_empty());
    assert_eq!(kugou.requests().len(), 2);
}

#[tokio::test]
async fn a_rate_limited_source_does_not_block_other_sources() {
    let dir = tempfile::tempdir().unwrap();
    let lrclib = Server::new(vec![(429, "busy".into())]);
    let kugou = Server::new(vec![
        success(json!({"status":200,"candidates":[kugou_record()]})),
        success(kugou_lyrics()),
    ]);
    let netease = Server::new(vec![]);
    let service = multi_service(&lrclib, &kugou, &netease, cache(&dir));
    let song = song();
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
    assert_eq!(
        outcome.document.unwrap().origin,
        crate::lyrics::LyricsOrigin::Kugou
    );
    assert!(service.cache.automatic(&Query::for_song(&song)).is_none());
    assert_eq!(kugou.requests().len(), 2);
}

#[tokio::test]
async fn manual_search_keeps_other_sources_results_and_reports_partial_failures() {
    let dir = tempfile::tempdir().unwrap();
    let lrclib = Server::new(vec![(500, "offline".into())]);
    let kugou = Server::with_handler(|request| {
        if request.url.path() == "/search" {
            success(json!({"status":200,"candidates":[kugou_record()]}))
        } else {
            success(kugou_lyrics())
        }
    });
    let netease = Server::new(vec![
        success(
            json!({"code":200,"result":{"songs":[{"id":7,"name":"Test","artists":[{"name":"Artist"}],"duration":223000}]}}),
        ),
        success(json!({"code":200,"lrc":{"lyric":"[00:01]test"}})),
    ]);
    let mut service = multi_service(&lrclib, &kugou, &netease, cache(&dir));
    service.settings.policy = LyricsPolicy::Manual;
    let query = Query::for_song(&song());
    let outcome = service.search(&query, None).await.unwrap();
    assert_eq!(outcome.records.len(), 2);
    assert_eq!(
        outcome.failures,
        [(ProviderId::Lrclib, ProviderError::Unavailable)]
    );
    assert_ne!(outcome.records[0].source, outcome.records[1].source);
    assert_eq!(outcome.records[0].id, outcome.records[1].id);
    let outcome = service
        .search(&query, Some(ProviderId::Kugou))
        .await
        .unwrap();
    assert_eq!(outcome.records.len(), 1);
    assert_eq!(outcome.records[0].source, ProviderId::Kugou);
    assert!(outcome.failures.is_empty());
    assert_eq!(lrclib.requests().len(), 1);
    assert_eq!(netease.requests().len(), 2);
    service.settings.enabled = false;
    assert_eq!(
        service.search(&query, None).await.unwrap_err(),
        ProviderError::Unavailable
    );
    assert_eq!(kugou.requests().len(), 4);
}

#[tokio::test]
async fn a_kugou_pin_works_offline_and_recovery_uses_its_source_and_query() {
    let dir = tempfile::tempdir().unwrap();
    let lrclib = Server::new(vec![]);
    let kugou = Server::new(vec![
        success(json!({"status":200,"candidates":[kugou_record()]})),
        success(kugou_lyrics()),
    ]);
    let netease = Server::new(vec![]);
    let song = song();
    let cache = cache(&dir);
    let mut document = Document::online(super::super::provider::Record {
        source: ProviderId::Kugou,
        id: 7,
        ..Default::default()
    });
    document.query = Some(Query::for_song(&song));
    document.offset_ms = 750;
    document.plain = true;
    cache
        .remember(&song.media_key().unwrap(), &document)
        .unwrap();
    let mut service = multi_service(&lrclib, &kugou, &netease, cache);
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
    let document = outcome.document.unwrap();
    assert_eq!(document.origin, crate::lyrics::LyricsOrigin::Kugou);
    assert!(document.manual);
    assert_eq!(document.offset_ms, 750);
    service.settings.enabled = false;
    service.settings.sources.clear();
    let document = service
        .load(
            &song,
            &song.media_key().unwrap(),
            None,
            CapabilitySupport::Unsupported,
            false,
            |_, _| {},
        )
        .await
        .document
        .unwrap();
    assert_eq!(document.origin, crate::lyrics::LyricsOrigin::Kugou);
    assert!(document.plain);
    assert_eq!(kugou.requests().len(), 2);
    assert!(lrclib.requests().is_empty());
}

#[tokio::test]
async fn complete_misses_fall_through_to_netease_and_cache_each_source() {
    let dir = tempfile::tempdir().unwrap();
    let lrclib = Server::new(vec![(404, "missing".into()), success(json!([]))]);
    let kugou = Server::new(vec![success(json!({"status":200,"candidates":[]}))]);
    let netease = Server::new(vec![
        success(
            json!({"code":200,"result":{"songs":[{"id":7,"name":"Test","artists":[{"name":"Artist"}],"duration":223000}]}}),
        ),
        success(json!({"code":200,"lrc":{"lyric":"[00:01]test"}})),
    ]);
    let service = multi_service(&lrclib, &kugou, &netease, cache(&dir));
    let song = song();
    let query = Query::for_song(&song);
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
        assert_eq!(
            outcome.document.unwrap().origin,
            crate::lyrics::LyricsOrigin::Netease
        );
    }
    for id in [ProviderId::Lrclib, ProviderId::Kugou] {
        assert!(service.cache.automatic_from(id, &query).unwrap().is_none());
    }
    assert_eq!(
        service
            .cache
            .automatic_from(ProviderId::Netease, &query)
            .unwrap()
            .unwrap()
            .id,
        7
    );
    assert_eq!(lrclib.requests().len(), 2);
    assert_eq!(kugou.requests().len(), 1);
    assert_eq!(netease.requests().len(), 2);
}

#[tokio::test]
async fn failed_candidate_downloads_are_never_negative_cached_or_auto_selected() {
    let dir = tempfile::tempdir().unwrap();
    let lrclib = Server::new(vec![]);
    let kugou = Server::new(vec![
        success(
            json!({"status":200,"candidates":[kugou_record(), {"id":"8", "song":"Test", "singer":"Artist", "duration":223000, "accesskey":"temporary-key"}]}),
        ),
        success(kugou_lyrics()),
        (500, "failed".into()),
    ]);
    let netease = Server::new(vec![
        success(
            json!({"code":200,"result":{"songs":[{"id":7,"name":"Test","artists":[{"name":"Artist"}],"duration":223000}, {"id":8,"name":"Test","artists":[{"name":"Artist"}],"duration":223000}]}}),
        ),
        success(json!({"code":200,"nolyric":true})),
        (404, "failed".into()),
    ]);
    let mut service = multi_service(&lrclib, &kugou, &netease, cache(&dir));
    service.settings.sources = vec![ProviderId::Kugou, ProviderId::Netease];
    let song = song();
    let query = Query::for_song(&song);
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
    assert_eq!(outcome.status, LyricsStatus::Unavailable);
    assert!(outcome.document.is_none());
    for id in service.settings.enabled_sources() {
        assert!(service.cache.automatic_from(id, &query).is_none());
    }
    assert!(lrclib.requests().is_empty());
}

#[tokio::test]
async fn no_enabled_sources_or_a_disabled_pin_never_issue_online_requests() {
    let dir = tempfile::tempdir().unwrap();
    let lrclib = Server::new(vec![]);
    let kugou = Server::new(vec![]);
    let netease = Server::new(vec![]);
    let mut service = multi_service(&lrclib, &kugou, &netease, cache(&dir));
    let song = song();
    let media = song.media_key().unwrap();
    service
        .cache
        .remember(
            &media,
            &Document::online(super::super::provider::Record {
                source: ProviderId::Kugou,
                id: 7,
                ..Default::default()
            }),
        )
        .unwrap();
    service.settings.sources.clear();
    let outcome = service
        .load(
            &song,
            &media,
            None,
            CapabilitySupport::Unsupported,
            false,
            |_, _| {},
        )
        .await;
    assert_eq!(outcome.status, LyricsStatus::SourcesNeeded);
    assert!(outcome.document.is_none());
    assert_eq!(
        service
            .search(&Query::for_song(&song), None)
            .await
            .unwrap_err(),
        ProviderError::Unavailable
    );
    assert!(lrclib.requests().is_empty());
    assert!(kugou.requests().is_empty());
    assert!(netease.requests().is_empty());
}

#[tokio::test]
async fn cancelling_aggregate_search_closes_every_pending_provider_request() {
    use tokio::io::AsyncReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (started, mut requests) = tokio::sync::mpsc::channel(3);
    let connections = tokio::spawn(async move {
        let mut tasks = tokio::task::JoinSet::new();
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().await.unwrap();
            let started = started.clone();
            tasks.spawn(async move {
                let mut header = Vec::new();
                while !header.ends_with(b"\r\n\r\n") {
                    header.push(stream.read_u8().await.unwrap());
                    assert!(header.len() < 16 * 1024);
                }
                started.send(()).await.unwrap();
                let mut buffer = [0];
                assert_eq!(stream.read(&mut buffer).await.unwrap(), 0);
            });
        }
        while let Some(result) = tasks.join_next().await {
            result.unwrap();
        }
    });
    let dir = tempfile::tempdir().unwrap();
    let service = LyricsService {
        settings: LyricsSettings {
            enabled: true,
            sources: ProviderId::ALL.to_vec(),
            ..Default::default()
        },
        provider: Ok(vec![
            Provider::Lrclib(crate::lyrics::lrclib::Lrclib::test_client(base.clone())),
            Provider::Kugou(crate::lyrics::kugou::Kugou::test_client(base.clone())),
            Provider::Netease(crate::lyrics::netease::Netease::test_client(base)),
        ]),
        cache: cache(&dir),
    };
    let search = tokio::spawn(async move { service.search(&Query::for_song(&song()), None).await });
    for _ in 0..3 {
        tokio::time::timeout(Duration::from_secs(5), requests.recv())
            .await
            .unwrap()
            .unwrap();
    }
    search.abort();
    assert!(search.await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(5), connections)
        .await
        .unwrap()
        .unwrap();
}
