use super::*;
use crate::source::bilibili::tests::{session, Fixture};
use serde_json::json;

fn server_clock() -> ServerClock {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::DATE,
        HeaderValue::from_static("Wed, 01 Jan 2025 00:00:00 GMT"),
    );
    ServerClock::from_headers(&headers, Instant::now(), SystemTime::now()).unwrap()
}

#[test]
fn address_lifetime_uses_server_time_even_when_local_clock_is_fast_or_slow() {
    let fixture = Fixture::new(|_| panic!("selection needs no HTTP"));
    let provider = fixture.provider();
    let received = Instant::now();
    let wall_received = SystemTime::now();
    // Simulate server dates far behind/ahead of the machine's wall clock.
    for date in [
        "Wed, 01 Jan 2025 00:00:00 GMT",
        "Fri, 01 Jan 2100 00:00:00 GMT",
    ] {
        let mut headers = HeaderMap::new();
        headers.insert(header::DATE, HeaderValue::from_str(date).unwrap());
        let clock = ServerClock::from_headers(&headers, received, wall_received).unwrap();
        let seconds = clock.time.duration_since(UNIX_EPOCH).unwrap().as_secs();
        let mut value = audio(
            1,
            192000,
            "mp4a.40.2",
            &format!("https://a.bilivideo.com/audio?deadline={}", seconds + 7200),
        );
        value["backupUrl"] = json!([format!(
            "https://a.bilivideo.com/backup?deadline={}",
            seconds + 3600
        )]);
        let resources = select_candidates(
            &provider,
            &loaded_song(&provider),
            None,
            serde_json::from_value(json!({"dash":{"audio":[value]}})).unwrap(),
            Some(clock),
            received,
        )
        .unwrap();
        let http = &resources[0].http;
        assert_eq!(
            http.expires_at(),
            Some(received + Duration::from_secs(7200))
        );
        assert_eq!(
            resources[0].backups[0].expires_at(),
            Some(received + Duration::from_secs(3600))
        );
        assert!(!http.expired_at(
            received + Duration::from_secs(7199),
            wall_received + Duration::from_secs(7199),
        ));
        assert!(http.expired_at(received + Duration::from_secs(7200), wall_received,));
        // Suspend advances wall time even when the monotonic clock stands still.
        assert!(http.expired_at(received, wall_received + Duration::from_secs(7200),));
    }
}

#[tokio::test]
async fn missing_or_invalid_server_date_leaves_expiry_to_the_cdn_probe() {
    for date in [None, Some("invalid")] {
        let fixture = Fixture::with_response(move |path, _| {
            if path.starts_with("/x/player/playurl") {
                let mut value = audio(
                    1,
                    192000,
                    "mp4a.40.2",
                    "https://a.bilivideo.com/audio?deadline=1",
                );
                value["backupUrl"] = json!([]);
                let body = json!({"code":0,"data":{"dash":{"audio":[value]}}}).to_string();
                let date = date.map_or(String::new(), |date| format!("Date: {date}\r\n"));
                format!("HTTP/1.1 200 OK\r\n{date}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes()
            } else {
                assert!(path.starts_with("/audio"));
                response(206, "audio/mp4", b"a")
            }
        });
        let provider = fixture.provider();
        let resource = provider
            .resolve_playback(&loaded_song(&provider))
            .await
            .unwrap();
        assert!(resource.http.expires_at().is_none());
        assert_eq!(fixture.paths.lock().unwrap().len(), 2);
    }
}

#[tokio::test]
async fn only_expired_addresses_report_expiry_without_opening_the_cdn() {
    let fixture = Fixture::with_response(|path, _| {
        assert!(path.starts_with("/x/player/playurl"));
        let mut value = audio(
            1,
            192000,
            "mp4a.40.2",
            "https://a.bilivideo.com/audio?deadline=1",
        );
        value["backupUrl"] = json!([]);
        json_response(json!({"code":0,"data":{"dash":{"audio":[value]}}}))
    });
    let provider = fixture.provider();
    assert_eq!(
        provider
            .resolve_playback(&loaded_song(&provider))
            .await
            .unwrap_err(),
        BilibiliError::ResourceExpired
    );
    assert_eq!(fixture.paths.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn many_backups_cannot_prevent_probing_a_lower_audio_format() {
    let fixture = Fixture::with_response(|path, _| {
        if path.starts_with("/x/player/playurl") {
            let mut high = audio(1, 192000, "mp4a.40.2", "https://a.bilivideo.com/high");
            high["backupUrl"] = json!((1..=8)
                .map(|i| format!("https://a.bilivideo.com/high{i}"))
                .collect::<Vec<_>>());
            let mut low = audio(2, 96000, "mp4a.40.2", "https://a.bilivideo.com/low");
            low["backupUrl"] = json!([]);
            json_response(json!({"code":0,"data":{"dash":{"audio":[low,high]}}}))
        } else if path.starts_with("/high") {
            response(403, "text/plain", b"denied")
        } else {
            assert_eq!(path, "/low");
            response(206, "audio/mp4", b"a")
        }
    });
    let provider = fixture.provider();
    let resource = provider
        .resolve_playback(&loaded_song(&provider))
        .await
        .unwrap();
    assert_eq!(resource.quality.bandwidth, Some(96000));
    assert_eq!(
        &fixture.paths.lock().unwrap()[1..],
        &["/high", "/high1", "/high2", "/low"]
    );
}

fn loaded_song(provider: &BilibiliProvider) -> Song {
    Song {
        duration: Some(100),
        bilibili_part: Some(crate::models::BilibiliPart {
            bvid: "BV1GJ411x7h7".into(),
            cid: 10,
            index: 1,
            count: 1,
            title: "Part".into(),
            uploader: "Uploader".into(),
        }),
        ..fixture_song(provider)
    }
}

#[tokio::test]
async fn loaded_parts_use_playurl_duration_without_requesting_view_and_keep_all_backups() {
    let fixture = Fixture::with_response(|path, _| {
        if path.starts_with("/x/player/playurl") {
            let mut value = audio(
                1,
                192000,
                "mp4a.40.2",
                "https://a.bilivideo.com/primary?deadline=4102444800",
            );
            value["backupUrl"] = json!((1..=5)
                .map(|i| format!("https://a.bilivideo.com/backup{i}?deadline=4102444700"))
                .collect::<Vec<_>>());
            json_response(
                json!({"code":0,"data":{"timelength":58250,"dash":{"duration":59,"audio":[value]}}}),
            )
        } else {
            assert!(path.starts_with("/primary"));
            response(206, "audio/mp4", b"a")
        }
    });
    let provider = fixture.provider();
    let resource = provider
        .resolve_playback(&loaded_song(&provider))
        .await
        .unwrap();
    assert_eq!(resource.duration, Some(Duration::from_millis(58250)));
    assert_eq!(resource.backups.len(), 5);
    let primary = resource.http.expires_at().unwrap();
    let backup = resource.backups[0].expires_at().unwrap();
    assert_eq!(primary.duration_since(backup), Duration::from_secs(100));
    // Fixture server date is 2025; local wall time is irrelevant.
    assert!(primary > Instant::now() + Duration::from_secs(3600));
    assert_eq!(fixture.paths.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn mismatched_loaded_identity_is_rejected_before_any_request() {
    let fixture = Fixture::new(|_| panic!("invalid part must not issue a request"));
    let provider = fixture.provider();
    let mut song = loaded_song(&provider);
    song.bilibili_part.as_mut().unwrap().cid = 99;
    assert_eq!(
        provider.resolve_playback(&song).await.unwrap_err(),
        BilibiliError::InvalidResponse
    );
    assert!(fixture.paths.lock().unwrap().is_empty());
}

#[tokio::test]
async fn audio_probe_follows_same_origin_redirects_without_cookies_or_signed_referer() {
    let fixture = Fixture::with_response(|path, headers| {
        if path.starts_with("/x/player/playurl") {
            json_response(
                json!({"code":0,"data":{"dash":{"audio":[audio(1,192000,"mp4a.40.2","https://a.bilivideo.com/redirect?signature=private")]}}}),
            )
        } else {
            assert!(!headers.contains_key("cookie"));
            assert_eq!(headers["referer"], REFERER);
            assert_eq!(headers["range"], "bytes=0-0");
            if path.starts_with("/redirect?") {
                b"HTTP/1.1 302 Found\r\nLocation: /audio\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
            } else {
                assert_eq!(path, "/audio");
                response(206, "audio/mp4", b"a")
            }
        }
    });
    let provider = fixture.provider();
    provider
        .resolve_playback(&loaded_song(&provider))
        .await
        .unwrap();
    assert_eq!(fixture.paths.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn account_redirect_and_risk_control_never_open_audio() {
    for risk_control in [false, true] {
        let fixture = Fixture::with_response(move |path, _| {
            assert!(path.starts_with("/x/player/playurl"));
            if risk_control {
                json_response(json!({"code":-352,"data":null}))
            } else {
                b"HTTP/1.1 302 Found\r\nLocation: /unexpected\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
            }
        });
        let provider = fixture.provider();
        assert_eq!(
            provider
                .resolve_playback(&loaded_song(&provider))
                .await
                .unwrap_err(),
            if risk_control {
                BilibiliError::RateLimited
            } else {
                BilibiliError::Http(302)
            }
        );
        assert_eq!(fixture.paths.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn resolution_deadline_keeps_the_last_cdn_failure() {
    let fixture = Fixture::with_response(|path, _| {
        if path.starts_with("/x/player/playurl") {
            json_response(
                json!({"code":0,"data":{"dash":{"audio":[audio(1,192000,"mp4a.40.2","https://a.bilivideo.com/primary")]}}}),
            )
        } else if path == "/primary" {
            response(403, "text/plain", b"denied")
        } else {
            std::thread::sleep(Duration::from_millis(300));
            response(206, "audio/mp4", b"a")
        }
    });
    let provider = fixture.provider();
    let error = resolve_before(
        &provider,
        &loaded_song(&provider),
        tokio::time::Instant::now() + Duration::from_millis(200),
    )
    .await
    .unwrap_err();
    assert_eq!(error, BilibiliError::Http(403));
}

#[test]
fn akamai_allowlist_accepts_only_bilibili_upos_hosts() {
    assert!(safe_audio_url("https://upos-hz-mirrorakam.akamaized.net/audio").is_some());
    for host in [
        "other-customer.akamaized.net",
        "upos-hz-mirrorakam.akamaized.net.evil.test",
        "evil.upos-hz-mirrorakam.akamaized.net",
        "upos--mirrorakam.akamaized.net",
    ] {
        assert!(
            safe_audio_url(&format!("https://{host}/audio")).is_none(),
            "{host}"
        );
    }
}

fn response(status: u16, mime: &str, body: &[u8]) -> Vec<u8> {
    let mut response = format!("HTTP/1.1 {status} Fixture\r\nDate: Wed, 01 Jan 2025 00:00:00 GMT\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes();
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
        crate::models::network::NetworkRoute::Direct,
    )
    .unwrap();
    let song = Song {
        source: Some(provider.id.clone()),
        id: "BV1GJ411x7h7:10".into(),
        ..Default::default()
    };
    let data = serde_json::from_value(json!({"dash":{"audio":[audio(1,64000,"mp4a.40.2","https://a.bilivideo.com/a?deadline=4102444800"),audio(3,320000,"mp4a.40.5","https://a.bilivideo.com/he"),audio(2,192000,"mp4a.40.2","https://a.bilivideo.com/b?deadline=4102444800")],"video":[{"baseUrl":"https://evil.test/video"}]}})).unwrap();
    let resource = select_candidates(
        &provider,
        &song,
        Some(Duration::from_secs(100)),
        data,
        Some(server_clock()),
        Instant::now(),
    )
    .unwrap()
    .remove(0);
    assert_eq!(resource.quality.bandwidth, Some(192000));
    assert_eq!(resource.quality.codec.as_deref(), Some("mp4a.40.2"));
    assert_eq!(resource.backups.len(), 1);
    assert_eq!(resource.duration, Some(Duration::from_secs(100)));
    assert!(matches!(
        resource.http.cache_policy(),
        CachePolicy::Temporary { .. }
    ));
    assert!(!resource.http.headers().contains_key(header::COOKIE));
    assert!(resource.http.expires_at().is_some());
    assert!(!format!("{resource:?}").contains("deadline="));
}

#[test]
fn video_only_and_unverified_codecs_are_unsupported() {
    let provider = BilibiliProvider::new(
        crate::source::SourceId::new(crate::source::ProviderKind::Bilibili, "test"),
        session(),
        crate::models::network::NetworkRoute::Direct,
    )
    .unwrap();
    let song = Song::default();
    for data in [
        json!({"durl":[{"url":"https://a.bilivideo.com/video"}]}),
        json!({"dash":{"audio":[audio(1,64000,"mp4a.40.5","https://a.bilivideo.com/audio")]}}),
    ] {
        assert_eq!(
            select_candidates(
                &provider,
                &song,
                Some(Duration::from_secs(100)),
                serde_json::from_value(data).unwrap(),
                Some(server_clock()),
                Instant::now()
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
        crate::models::network::NetworkRoute::Direct,
    )
    .unwrap();
    let data = serde_json::from_value(json!({"dash":{"audio":[{"id":1,"bandwidth":192000,"codecs":"mp4a.40.2","mimeType":"audio/mp4","baseUrl":"https://a.bilivideo.com/a?deadline=1"}]}})).unwrap();
    assert_eq!(
        select_candidates(
            &provider,
            &Song::default(),
            Some(Duration::from_secs(100)),
            data,
            Some(server_clock()),
            Instant::now()
        )
        .unwrap_err(),
        BilibiliError::ResourceExpired
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
