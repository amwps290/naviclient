use super::*;
use crate::test_support::http::{success, Server};
use serde_json::json;

fn candidate(id: u64) -> Value {
    json!({"id":id.to_string(),"song":"稻香","singer":"周杰伦","duration":222563,"accesskey":"temporary-key"})
}
fn lyric(id: u64) -> Value {
    json!({"status":200,"id":id.to_string(),"content":STANDARD.encode("[00:01.20]test"),"fmt":"lrc","charset":"utf8"})
}

#[tokio::test]
async fn search_decodes_lrc_and_fetch_reacquires_the_candidate_key() {
    let server = Server::new(vec![
        success(json!({"status":200,"candidates":[candidate(7)]})),
        success(lyric(7)),
        success(json!({"status":200,"candidates":[candidate(7)]})),
        success(lyric(7)),
    ]);
    let client = Kugou::test_client(server.url(""));
    let query = Query {
        title: "稻香 & +".into(),
        artist: "周杰伦".into(),
        duration: Some(223),
        ..Default::default()
    };
    let records = client.search(&query).await.unwrap();
    assert_eq!(records[0].source, ProviderId::Kugou);
    assert_eq!(records[0].duration, 222.563);
    assert_eq!(records[0].lyrics().lines[0].start_ms, Some(1200));
    let stored = serde_json::to_string(&records[0]).unwrap();
    assert!(!stored.contains("temporary-key"));
    assert!(!stored.contains("accesskey"));
    assert_eq!(client.fetch(7, Some(&query)).await.unwrap().unwrap().id, 7);
    let requests = server.requests();
    assert_eq!(
        requests
            .iter()
            .map(|request| request.url.path())
            .collect::<Vec<_>>(),
        ["/search", "/download", "/search", "/download"]
    );
    let params: std::collections::BTreeMap<_, _> =
        requests[0].url.query_pairs().into_owned().collect();
    assert_eq!(params["keyword"], "周杰伦 稻香 & +");
    assert_eq!(params["duration"], "223000");
    for request in requests {
        assert!(!request.headers.contains_key("cookie"));
        assert!(!request.headers.contains_key("authorization"));
        assert!(request.body.is_empty());
    }
}

#[tokio::test]
async fn malformed_downloads_and_missing_keys_are_errors_instead_of_misses() {
    for download in [
        json!({"status":200,"content":"not base64!"}),
        json!({"status":200,"content":STANDARD.encode([255u8])}),
        lyric(8),
    ] {
        let server = Server::new(vec![
            success(json!({"status":200,"candidates":[candidate(7)]})),
            success(download),
        ]);
        assert_eq!(
            Kugou::test_client(server.url(""))
                .search(&Query::default())
                .await
                .unwrap_err(),
            ProviderError::InvalidResponse
        );
    }
    let server = Server::new(vec![success(
        json!({"status":200,"candidates":[{"id":"7","song":"test","singer":"artist"}]}),
    )]);
    assert_eq!(
        Kugou::test_client(server.url(""))
            .search(&Query::default())
            .await
            .unwrap_err(),
        ProviderError::InvalidResponse
    );
    let mixed = Server::new(vec![success(
        json!({"status":200,"candidates":[candidate(7), {}]}),
    )]);
    assert_eq!(
        Kugou::test_client(mixed.url(""))
            .search(&Query::default())
            .await
            .unwrap_err(),
        ProviderError::InvalidResponse
    );
    assert_eq!(mixed.requests().len(), 1);
}

#[tokio::test]
async fn no_candidates_does_not_mean_instrumental_and_five_downloads_are_the_limit() {
    let server = Server::with_handler(|request| {
        if request.url.path() == "/search" {
            success(json!({"status":200,"candidates":(1..=20).map(candidate).collect::<Vec<_>>() }))
        } else {
            let id = request
                .url
                .query_pairs()
                .find(|(key, _)| key == "id")
                .unwrap()
                .1
                .parse()
                .unwrap();
            success(lyric(id))
        }
    });
    let records = Kugou::test_client(server.url(""))
        .search(&Query::default())
        .await
        .unwrap();
    assert_eq!(records.len(), 5);
    assert_eq!(server.requests().len(), 6);
    let empty = Server::new(vec![success(json!({"status":200,"candidates":[]}))]);
    assert!(Kugou::test_client(empty.url(""))
        .search(&Query::default())
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
#[ignore = "uses the live anonymous Kugou lyrics service"]
async fn live_kugou_search_and_fetch_chinese_lyrics() {
    let client = Kugou::new(&NetworkRoute::Direct, RequestGate::default()).unwrap();
    let query = Query {
        title: "稻香".into(),
        artist: "周杰伦".into(),
        duration: Some(223),
        ..Default::default()
    };
    let records = client.search(&query).await.unwrap();
    let record = records
        .iter()
        .find(|record| matching::assess(&query, record).trusted)
        .expect("expected a synced candidate with matching Chinese metadata and duration");
    assert!(record.lyrics().is_synced());
    let fetched = client
        .fetch(record.id, Some(&query))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fetched.source, ProviderId::Kugou);
    assert_eq!(fetched.id, record.id);
    assert!(fetched.lyrics().is_synced());
}
