use super::*;
use crate::source::bilibili::tests::{session, Fixture};
use serde_json::json;

fn response(status: u16, mime: &str, body: &[u8]) -> Vec<u8> {
    let mut response = format!("HTTP/1.1 {status} Fixture\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
    response.extend_from_slice(body);
    response
}

fn json_response(value: serde_json::Value) -> Vec<u8> {
    response(200, "application/json", value.to_string().as_bytes())
}

fn fixture_song(provider: &BilibiliProvider) -> Song {
    Song {
        source: Some(provider.id.clone()),
        id: "BV1GJ411x7h7:10".into(),
        ..Default::default()
    }
}

fn audio(id: u64, bandwidth: u64, codec: &str, url: &str) -> serde_json::Value {
    json!({"id":id,"bandwidth":bandwidth,"codecs":codec,"mimeType":"audio/mp4","baseUrl":url,"backupUrl":["https://backup.bilivideo.com/audio?deadline=4102444800"]})
}

#[test]
fn selects_highest_aac_lc_and_isolates_secrets_and_cache() {
    let provider = BilibiliProvider::new(
        crate::source::SourceId::new(crate::source::ProviderKind::Bilibili, "test"),
        session(),
        None,
    )
    .unwrap();
    let song = Song {
        source: Some(provider.id.clone()),
        id: "BV1GJ411x7h7:10".into(),
        ..Default::default()
    };
    let data = serde_json::from_value(json!({"dash":{"audio":[audio(1,64000,"mp4a.40.2","https://a.bilivideo.com/a?deadline=4102444800"),audio(3,320000,"mp4a.40.5","https://a.bilivideo.com/he"),audio(2,192000,"mp4a.40.2","https://a.bilivideo.com/b?deadline=4102444800")],"video":[{"baseUrl":"https://evil.test/video"}]}})).unwrap();
    let resource = select(&provider, &song, 100, data, SystemTime::now()).unwrap();
    assert_eq!(resource.quality.bandwidth, Some(192000));
    assert_eq!(resource.quality.codec.as_deref(), Some("mp4a.40.2"));
    assert_eq!(resource.backups.len(), 1);
    assert_eq!(resource.duration, Some(Duration::from_secs(100)));
    assert!(matches!(
        resource.http.cache_policy(),
        CachePolicy::Temporary { .. }
    ));
    assert!(!resource.http.headers().contains_key(header::COOKIE));
    assert!(resource.expires_at.is_some());
    assert!(!format!("{resource:?}").contains("deadline="));
}

#[test]
fn video_only_and_unverified_codecs_are_unsupported() {
    let provider = BilibiliProvider::new(
        crate::source::SourceId::new(crate::source::ProviderKind::Bilibili, "test"),
        session(),
        None,
    )
    .unwrap();
    let song = Song::default();
    for data in [
        json!({"durl":[{"url":"https://a.bilivideo.com/video"}]}),
        json!({"dash":{"audio":[audio(1,64000,"mp4a.40.5","https://a.bilivideo.com/audio")]}}),
    ] {
        assert_eq!(
            select(
                &provider,
                &song,
                100,
                serde_json::from_value(data).unwrap(),
                SystemTime::now()
            )
            .unwrap_err(),
            BilibiliError::UnsupportedAudio
        );
    }
}

#[test]
fn duplicate_api_naming_styles_and_null_backups_are_supported() {
    let value = json!({"id":30280,"bandwidth":192000,"codecs":"mp4a.40.2","mimeType":"audio/mp4","mime_type":"audio/mp4","baseUrl":"https://a.bilivideo.com/audio","base_url":"https://a.bilivideo.com/audio","backupUrl":null,"backup_url":null});
    let audio: Audio = serde_json::from_value(value).unwrap();
    assert_eq!(audio.mime, "audio/mp4");
    assert!(audio.backups.is_empty());
}

#[test]
fn unsafe_and_expired_cdn_urls_are_discarded() {
    for url in [
        "https://127.0.0.1/audio",
        "https://bilivideo.com.evil.test/a",
        "https://user@a.bilivideo.com/a",
        "https://a.bilivideo.com:123/a",
        "https://a.bilivideo.com/a?deadline=invalid",
    ] {
        assert!(safe_audio_url(url).is_none());
    }
    let provider = BilibiliProvider::new(
        crate::source::SourceId::new(crate::source::ProviderKind::Bilibili, "test"),
        session(),
        None,
    )
    .unwrap();
    let data = serde_json::from_value(json!({"dash":{"audio":[{"id":1,"bandwidth":192000,"codecs":"mp4a.40.2","mimeType":"audio/mp4","baseUrl":"https://a.bilivideo.com/a?deadline=1"}]}})).unwrap();
    assert_eq!(
        select(&provider, &Song::default(), 100, data, SystemTime::now()).unwrap_err(),
        BilibiliError::UnsupportedAudio
    );
}

#[tokio::test]
async fn mismatched_part_never_requests_audio_or_video() {
    let fixture = Fixture::new(|_| crate::source::bilibili::tests::video("BV1GJ411x7h7", &[10]));
    let provider = fixture.provider();
    let song = Song {
        source: Some(provider.id.clone()),
        id: "BV1GJ411x7h7:99".into(),
        ..Default::default()
    };
    assert_eq!(
        provider.resolve_playback(&song).await.unwrap_err(),
        BilibiliError::Unavailable
    );
    assert!(fixture
        .paths
        .lock()
        .unwrap()
        .iter()
        .all(|path| path.contains("/view")));
}

#[tokio::test]
async fn rejected_primary_uses_backup_with_range_and_without_account_cookies() {
    let fixture = Fixture::with_response(|path, headers| {
        if path.starts_with("/x/web-interface/view") {
            assert!(headers["cookie"].contains("SESSDATA=fake"));
            json_response(crate::source::bilibili::tests::video("BV1GJ411x7h7", &[10]))
        } else if path.starts_with("/x/player/playurl") {
            assert!(path.contains("cid=10"));
            json_response(
                json!({"code":0,"data":{"dash":{"audio":[audio(1,192000,"mp4a.40.2","https://primary.bilivideo.com/primary")],"video":[{"baseUrl":"https://primary.bilivideo.com/video"}]}}}),
            )
        } else {
            assert!(!headers.contains_key("cookie"));
            assert_eq!(headers["range"], "bytes=0-0");
            assert_eq!(headers["referer"], REFERER);
            assert_eq!(headers["user-agent"], USER_AGENT);
            if path == "/primary" {
                response(403, "text/plain", b"denied")
            } else if path.starts_with("/audio") {
                response(206, "application/octet-stream", b"a")
            } else {
                panic!("unexpected media request: {path}")
            }
        }
    });
    let provider = fixture.provider();
    let resource = provider
        .resolve_playback(&fixture_song(&provider))
        .await
        .unwrap();
    assert!(resource.http.url().contains("backup.bilivideo.com"));
    assert_eq!(resource.quality.bandwidth, Some(192000));
    assert!(resource.backups.is_empty());
    let paths = fixture.paths.lock().unwrap();
    assert_eq!(paths.len(), 4);
    assert!(!paths.iter().any(|path| path.starts_with("/video")));
    assert_eq!(
        fixture
            .headers
            .lock()
            .unwrap()
            .iter()
            .filter(|headers| headers.contains_key("cookie"))
            .count(),
        2
    );
}

#[tokio::test]
async fn inaccessible_representation_falls_back_only_to_supported_audio() {
    let fixture = Fixture::with_response(|path, _| {
        if path.starts_with("/x/web-interface/view") {
            json_response(crate::source::bilibili::tests::video("BV1GJ411x7h7", &[10]))
        } else if path.starts_with("/x/player/playurl") {
            let mut high = audio(1, 192000, "mp4a.40.2", "https://a.bilivideo.com/high");
            let mut low = audio(2, 96000, "mp4a.40.2", "https://a.bilivideo.com/low");
            high["backupUrl"] = json!([]);
            low["backupUrl"] = json!([]);
            json_response(
                json!({"code":0,"data":{"dash":{"audio":[low,high,audio(3,320000,"mp4a.40.5","https://a.bilivideo.com/unsupported")]}}}),
            )
        } else if path == "/high" {
            response(403, "text/plain", b"denied")
        } else if path == "/low" {
            response(206, "audio/mp4", b"a")
        } else {
            panic!("unexpected request: {path}")
        }
    });
    let provider = fixture.provider();
    let resource = provider
        .resolve_playback(&fixture_song(&provider))
        .await
        .unwrap();
    assert_eq!(resource.quality.bandwidth, Some(96000));
    let paths = fixture.paths.lock().unwrap();
    assert_eq!(&paths[2..], &["/high", "/low"]);
}

#[tokio::test]
async fn rate_limit_stops_before_trying_another_address_or_representation() {
    let fixture = Fixture::with_response(|path, _| {
        if path.starts_with("/x/web-interface/view") {
            json_response(crate::source::bilibili::tests::video("BV1GJ411x7h7", &[10]))
        } else if path.starts_with("/x/player/playurl") {
            json_response(
                json!({"code":0,"data":{"dash":{"audio":[audio(1,192000,"mp4a.40.2","https://a.bilivideo.com/high"),audio(2,96000,"mp4a.40.2","https://a.bilivideo.com/low")]}}}),
            )
        } else {
            assert_eq!(path, "/high");
            response(429, "text/plain", b"limited")
        }
    });
    let provider = fixture.provider();
    assert_eq!(
        provider
            .resolve_playback(&fixture_song(&provider))
            .await
            .unwrap_err(),
        BilibiliError::RateLimited
    );
    assert_eq!(fixture.paths.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn api_auth_failure_and_video_only_responses_never_open_media() {
    for play in [
        json!({"code":-101,"data":null}),
        json!({"code":0,"data":{"durl":[{"url":"https://a.bilivideo.com/video"}],"dash":{"video":[{"baseUrl":"https://a.bilivideo.com/video"}]}}}),
    ] {
        let expected = if play["code"] == -101 {
            BilibiliError::NotAuthenticated
        } else {
            BilibiliError::UnsupportedAudio
        };
        let fixture = Fixture::new(move |path| {
            if path.starts_with("/x/web-interface/view") {
                crate::source::bilibili::tests::video("BV1GJ411x7h7", &[10])
            } else {
                assert!(path.starts_with("/x/player/playurl"));
                play.clone()
            }
        });
        let provider = fixture.provider();
        assert_eq!(
            provider
                .resolve_playback(&fixture_song(&provider))
                .await
                .unwrap_err(),
            expected
        );
        assert_eq!(fixture.paths.lock().unwrap().len(), 2);
    }
}

#[tokio::test]
async fn resolving_again_renews_signed_address_without_persisting_it_in_media_identity() {
    let serial = std::sync::atomic::AtomicU64::new(0);
    let fixture = Fixture::with_response(move |path, _| {
        if path.starts_with("/x/web-interface/view") {
            json_response(crate::source::bilibili::tests::video("BV1GJ411x7h7", &[10]))
        } else if path.starts_with("/x/player/playurl") {
            let revision = serial.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            json_response(
                json!({"code":0,"data":{"dash":{"audio":[audio(1,192000,"mp4a.40.2",&format!("https://a.bilivideo.com/audio?deadline=4102444800&signature={revision}"))]}}}),
            )
        } else {
            assert!(path.starts_with("/audio"));
            response(206, "audio/mp4", b"a")
        }
    });
    let provider = fixture.provider();
    let song = fixture_song(&provider);
    let first = provider.resolve_playback(&song).await.unwrap();
    let second = provider.resolve_playback(&song).await.unwrap();
    assert_ne!(first.http.url(), second.http.url());
    assert_eq!(first.media, second.media);
    assert_eq!(first.cache_key(), second.cache_key());
    assert!(!serde_json::to_string(&song).unwrap().contains("signature"));
}
