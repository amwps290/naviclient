use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::*;
use crate::test_support::http::{success, Server};

fn song() -> Song {
    Song {
        id: "song/曲目 & +".into(),
        artist: "Artist 艺人 & +".into(),
        title: "Title 曲名 / ?".into(),
        ..Default::default()
    }
}

fn api(server: &Server) -> OpenSubsonicProvider {
    OpenSubsonicProvider::new(&server.url(""), "listener", "fixture").unwrap()
}

fn ok(payload: Value) -> (u16, String) {
    let mut payload = payload.as_object().unwrap().clone();
    payload.insert("status".into(), json!("ok"));
    success(json!({"subsonic-response": payload}))
}

fn structured() -> (u16, String) {
    ok(json!({"lyricsList": {"structuredLyrics": [
        {"lang": "eng", "synced": true, "offset": -100,
         "displayArtist": "Display Artist", "displayTitle": "Display Title",
         "line": [{"start": 1000, "value": " original "}]},
        {"lang": "zho", "synced": true,
         "line": [{"start": 900, "value": " 翻译 "}]}
    ]}}))
}

fn plain() -> (u16, String) {
    ok(json!({"lyrics": {
        "artist": "Legacy Artist", "title": "Legacy Title",
        "value": " first line \n\n  \nsecond line "
    }}))
}

fn failures() -> [((u16, String), &'static str); 4] {
    [
        ((502, "gateway offline".into()), "HTTP 502"),
        ((200, "{".into()), "invalid JSON"),
        ((200, "{}".into()), "not a Subsonic response"),
        (
            success(json!({"subsonic-response": {
                "status": "failed", "error": {"code": 50, "message": "source unavailable"}
            }})),
            "Subsonic error 50: source unavailable",
        ),
    ]
}

fn assert_requests(server: &Server, endpoints: &[&str]) {
    let requests = server.requests();
    assert_eq!(requests.len(), endpoints.len());
    let song = song();
    for (request, endpoint) in requests.iter().zip(endpoints) {
        assert_eq!(request.method, "GET");
        assert!(request.body.is_empty());
        assert_eq!(request.url.path(), format!("/rest/{endpoint}.view"));
        let params: BTreeMap<_, _> = request
            .url
            .query_pairs()
            .into_owned()
            .filter(|(key, _)| !["u", "t", "s", "v", "c", "f"].contains(&key.as_str()))
            .collect();
        let expected = if *endpoint == "getLyricsBySongId" {
            BTreeMap::from([("id".into(), song.id.clone())])
        } else {
            BTreeMap::from([
                ("artist".into(), song.artist.clone()),
                ("title".into(), song.title.clone()),
            ])
        };
        assert_eq!(params, expected);
    }
}

fn assert_plain(lyrics: &Lyrics) {
    assert_eq!(lyrics.display_artist.as_deref(), Some("Legacy Artist"));
    assert_eq!(lyrics.display_title.as_deref(), Some("Legacy Title"));
    assert!(!lyrics.is_synced());
    assert_eq!(
        lyrics
            .lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>(),
        ["first line", "second line"]
    );
    assert!(lyrics.lines.iter().all(|line| line.translation.is_none()));
}

fn assert_empty(lyrics: &Lyrics) {
    assert!(lyrics.lines.is_empty());
    assert!(lyrics.display_artist.is_none());
    assert!(lyrics.display_title.is_none());
}

#[tokio::test]
async fn supported_and_unknown_capabilities_return_structured_lyrics_without_legacy_requests() {
    for support in [CapabilitySupport::Supported, CapabilitySupport::Unknown] {
        let server = Server::new(vec![structured()]);
        let lyrics = api(&server).lyrics(&song(), support).await.unwrap();
        assert_eq!(lyrics.display_artist.as_deref(), Some("Display Artist"));
        assert_eq!(lyrics.display_title.as_deref(), Some("Display Title"));
        assert!(lyrics.is_synced());
        assert_eq!(lyrics.lines.len(), 1);
        assert_eq!(lyrics.lines[0].start_ms, Some(900));
        assert_eq!(lyrics.lines[0].text, "original");
        assert_eq!(lyrics.lines[0].translation.as_deref(), Some("翻译"));
        assert_requests(&server, &["getLyricsBySongId"]);
    }
}

#[tokio::test]
async fn unsupported_capability_requests_only_legacy_lyrics_with_original_metadata() {
    let server = Server::new(vec![plain()]);
    let lyrics = api(&server)
        .lyrics(&song(), CapabilitySupport::Unsupported)
        .await
        .unwrap();
    assert_plain(&lyrics);
    assert_requests(&server, &["getLyrics"]);
}

#[tokio::test]
async fn structured_transport_and_api_failures_fall_back_once_to_legacy_lyrics() {
    for support in [CapabilitySupport::Supported, CapabilitySupport::Unknown] {
        for (failure, _) in failures() {
            let server = Server::new(vec![failure, plain()]);
            let lyrics = api(&server).lyrics(&song(), support).await.unwrap();
            assert_plain(&lyrics);
            assert_requests(&server, &["getLyricsBySongId", "getLyrics"]);
        }
    }
}

#[tokio::test]
async fn unusable_structured_payloads_fall_back_and_empty_legacy_payloads_return_empty_lyrics() {
    let unusable = [
        json!({}),
        json!({"lyricsList": {"structuredLyrics": []}}),
        json!({"lyricsList": {"structuredLyrics": [{"line": [{"value": "  "}]}]}}),
        json!({"lyricsList": {"structuredLyrics": "malformed"}}),
    ];
    for payload in unusable {
        let server = Server::new(vec![ok(payload), plain()]);
        let lyrics = api(&server)
            .lyrics(&song(), CapabilitySupport::Supported)
            .await
            .unwrap();
        assert_plain(&lyrics);
        assert_requests(&server, &["getLyricsBySongId", "getLyrics"]);
    }
    for payload in [
        json!({}),
        json!({"lyrics": {"value": " \n "}}),
        json!({"lyrics": {"value": 42}}),
    ] {
        // A successful legacy response without content remains an empty result,
        // even when the structured source failed.
        let server = Server::new(vec![failures()[0].0.clone(), ok(payload)]);
        let lyrics = api(&server)
            .lyrics(&song(), CapabilitySupport::Unknown)
            .await
            .unwrap();
        assert_empty(&lyrics);
        assert_requests(&server, &["getLyricsBySongId", "getLyrics"]);
    }
}

#[tokio::test]
async fn successful_structured_response_without_content_tolerates_legacy_failures() {
    for (failure, _) in failures() {
        let server = Server::new(vec![ok(json!({})), failure]);
        let lyrics = api(&server)
            .lyrics(&song(), CapabilitySupport::Supported)
            .await
            .unwrap();
        assert_empty(&lyrics);
        assert_requests(&server, &["getLyricsBySongId", "getLyrics"]);
    }
}

#[tokio::test]
async fn both_sources_failing_preserves_structured_error_with_legacy_context() {
    for (structured_failure, structured_message) in failures() {
        for (legacy_failure, legacy_message) in failures() {
            let server = Server::new(vec![structured_failure.clone(), legacy_failure]);
            let error = api(&server)
                .lyrics(&song(), CapabilitySupport::Unknown)
                .await
                .unwrap_err();
            let context = error.to_string();
            assert!(context.starts_with("legacy lyrics request also failed: "));
            assert!(context.contains(legacy_message));
            let source = error.chain().nth(1).unwrap().to_string();
            assert!(source.contains(structured_message), "{error:#}");
            assert_requests(&server, &["getLyricsBySongId", "getLyrics"]);
        }
    }
}

#[tokio::test]
async fn unsupported_capability_preserves_legacy_failure_without_structured_context() {
    for (failure, message) in failures() {
        let server = Server::new(vec![failure]);
        let error = api(&server)
            .lyrics(&song(), CapabilitySupport::Unsupported)
            .await
            .unwrap_err();
        assert!(error.to_string().contains(message));
        assert!(!error
            .to_string()
            .contains("legacy lyrics request also failed"));
        assert_requests(&server, &["getLyrics"]);
    }
}
