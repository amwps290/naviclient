use super::*;
use crate::test_support::http::{success, Server};
use serde_json::json;

fn song(id: u64) -> Value {
    json!({"id":id,"name":"十年","artists":[{"name":"陈奕迅"}],"album":{"name":"黑白灰"},"duration":205423})
}

#[tokio::test]
async fn search_hydrates_synced_lyrics_and_fetch_verifies_metadata() {
    let server = Server::new(vec![
        success(json!({"code":200,"result":{"songs":[song(7)]}})),
        success(json!({"code":200,"lrc":{"lyric":"[00:01.20]test"}})),
        success(json!({"code":200,"songs":[song(7)]})),
        success(json!({"code":200,"lrc":{"lyric":"[00:01.20]test"}})),
    ]);
    let client = Netease::test_client(server.url(""));
    let query = Query {
        title: "十年 & +".into(),
        artist: "陈奕迅".into(),
        ..Default::default()
    };
    let records = client.search(&query).await.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].source, ProviderId::Netease);
    assert_eq!(records[0].duration, 205.423);
    assert_eq!(records[0].lyrics().lines[0].start_ms, Some(1200));
    assert_eq!(client.fetch(7, None).await.unwrap().unwrap().id, 7);
    let requests = server.requests();
    assert_eq!(
        requests
            .iter()
            .map(|request| request.url.path())
            .collect::<Vec<_>>(),
        [
            "/api/search/get/web",
            "/api/song/lyric/v1",
            "/api/song/detail",
            "/api/song/lyric/v1"
        ]
    );
    assert_eq!(
        requests[0]
            .url
            .query_pairs()
            .find(|(key, _)| key == "s")
            .unwrap()
            .1,
        "十年 & + 陈奕迅"
    );
    for request in requests {
        assert!(!request.headers.contains_key("cookie"));
        assert!(!request.headers.contains_key("authorization"));
        assert!(request.body.is_empty());
    }
}

#[tokio::test]
async fn empty_lyrics_and_api_failures_are_distinct() {
    let server = Server::new(vec![
        success(json!({"code":200,"result":{"songCount":0}})),
        success(json!({"code":200,"result":{"songs":[song(7)]}})),
        success(json!({"code":200,"nolyric":true})),
        success(json!({"code":301})),
        success(json!({"code":200,"result":{"songs":[{}]}})),
    ]);
    let client = Netease::test_client(server.url(""));
    let query = Query {
        title: "十年".into(),
        ..Default::default()
    };
    assert!(client.search(&query).await.unwrap().is_empty());
    assert!(client.search(&query).await.unwrap().is_empty());
    assert_eq!(
        client.search(&query).await.unwrap_err(),
        ProviderError::Unavailable
    );
    assert_eq!(
        client.search(&query).await.unwrap_err(),
        ProviderError::InvalidResponse
    );
    let mixed = Server::new(vec![success(
        json!({"code":200,"result":{"songs":[song(7), {}]}}),
    )]);
    assert_eq!(
        Netease::test_client(mixed.url(""))
            .search(&query)
            .await
            .unwrap_err(),
        ProviderError::InvalidResponse
    );
    assert_eq!(mixed.requests().len(), 1);
}

#[tokio::test]
async fn hydration_is_bounded_and_plain_lyrics_remain_usable() {
    let server = Server::with_handler(|request| {
        if request.url.path().contains("search") {
            success(json!({"code":200,"result":{"songs":(1..=20).map(song).collect::<Vec<_>>()}}))
        } else {
            success(json!({"code":200,"lrc":{"lyric":"plain text"}}))
        }
    });
    let client = Netease::test_client(server.url(""));
    let records = client
        .search(&Query {
            title: "十年".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(records.len(), 5);
    assert!(!records[0].lyrics().is_synced());
    assert!(!records[0].instrumental);
    assert_eq!(server.requests().len(), 6);
}

#[tokio::test]
#[ignore = "uses the live anonymous NetEase lyrics service"]
async fn live_netease_search_and_fetch_chinese_lyrics() {
    let client = Netease::new(&NetworkRoute::Direct, RequestGate::default()).unwrap();
    let query = Query {
        title: "十年".into(),
        artist: "陈奕迅".into(),
        duration: Some(205),
        ..Default::default()
    };
    let records = client.search(&query).await.unwrap();
    let record = records
        .iter()
        .find(|record| matching::assess(&query, record).trusted)
        .expect("expected a synced candidate with matching Chinese metadata and duration");
    assert!(record.lyrics().is_synced());
    let fetched = client.fetch(record.id, None).await.unwrap().unwrap();
    assert_eq!(fetched.source, ProviderId::Netease);
    assert_eq!(fetched.id, record.id);
    assert!(fetched.lyrics().is_synced());
}

#[tokio::test]
async fn enhanced_lyrics_preserve_yrc_and_lrc_and_work_without_lrc() {
    for lrc in ["[00:01]fallback", ""] {
        let server = Server::new(vec![
            success(json!({"code":200,"songs":[song(7)]})),
            success(
                json!({"code":200,"lrc":{"lyric":lrc},"yrc":{"lyric":"[1000,1000](1000,500,0)你(1500,500,0)好"}}),
            ),
        ]);
        let record = Netease::test_client(server.url(""))
            .fetch(7, None)
            .await
            .unwrap()
            .unwrap();
        assert!(record.lyrics().has_word_timing());
        assert_eq!(record.lyrics().lines[0].text, "你好");
        assert_eq!(
            record.synced_lyrics.as_deref(),
            (!lrc.is_empty()).then_some(lrc)
        );
        let request = &server.requests()[1];
        assert_eq!(request.url.path(), "/api/song/lyric/v1");
        assert!(request
            .url
            .query_pairs()
            .any(|(key, value)| key == "yv" && value == "0"));
    }
}

#[tokio::test]
async fn enhanced_endpoint_failure_falls_back_to_legacy_but_rate_limits_do_not() {
    let server = Server::new(vec![
        success(json!({"code":200,"songs":[song(7)]})),
        (502, "offline".into()),
        success(json!({"code":200,"lrc":{"lyric":"[00:01]fallback"}})),
    ]);
    let record = Netease::test_client(server.url(""))
        .fetch(7, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.lyrics().lines[0].text, "fallback");
    assert_eq!(server.requests()[2].url.path(), "/api/song/lyric");
    let server = Server::new(vec![
        success(json!({"code":200,"songs":[song(7)]})),
        (429, "busy".into()),
    ]);
    assert!(matches!(
        Netease::test_client(server.url("")).fetch(7, None).await,
        Err(ProviderError::RateLimited { .. })
    ));
    assert_eq!(server.requests().len(), 2);
}

#[tokio::test]
#[ignore = "uses the live anonymous NetEase word lyrics service"]
async fn live_netease_word_lyrics() {
    let record = Netease::new(&NetworkRoute::Direct, RequestGate::default())
        .unwrap()
        .fetch(66842, None)
        .await
        .unwrap()
        .unwrap();
    let lyrics = record.lyrics();
    assert!(lyrics.has_word_timing());
    let timed_lines = lyrics
        .lines
        .iter()
        .filter_map(|line| line.word_timing.as_ref())
        .count();
    let segments: usize = lyrics
        .lines
        .iter()
        .filter_map(|line| line.word_timing.as_ref())
        .map(|words| words.segments.len())
        .sum();
    println!("verified YRC: {timed_lines} timed lines, {segments} timed segments");
    assert!(
        timed_lines >= 20 && segments >= 200,
        "expected the vocal timeline, not just a metadata cue"
    );
    assert!(lyrics
        .lines
        .iter()
        .filter_map(|line| line.word_timing.as_ref())
        .all(|words| words.is_valid()));
}
