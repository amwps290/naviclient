use super::*;
use crate::test_support::http::{success, Server};
use serde_json::json;

#[tokio::test]
async fn utf8_queries_and_record_ids_use_public_metadata_only() {
    let record = json!({"id":7,"trackName":"稻香","artistName":"周杰倫","albumName":"魔杰座","duration":223,"instrumental":false,"plainLyrics":"text","syncedLyrics":"[00:01.20]text"});
    let server = Server::new(vec![
        success(record.clone()),
        success(json!([record.clone()])),
        success(record),
    ]);
    let client = Lrclib::test_client(server.url(""));
    let query = Query {
        title: "稻香 & +".into(),
        artist: "周杰倫".into(),
        album: "魔杰座".into(),
        duration: Some(223),
    };
    assert!(client
        .lookup(&query)
        .await
        .unwrap()
        .unwrap()
        .lyrics()
        .is_synced());
    assert_eq!(client.search(&query).await.unwrap().len(), 1);
    assert_eq!(client.fetch(7, None).await.unwrap().unwrap().id, 7);
    let requests = server.requests();
    assert_eq!(
        requests.iter().map(|r| r.url.path()).collect::<Vec<_>>(),
        vec!["/api/get", "/api/search", "/api/get/7"]
    );
    let params: std::collections::BTreeMap<_, _> =
        requests[0].url.query_pairs().into_owned().collect();
    assert_eq!(params.len(), 4);
    assert_eq!(params["track_name"], query.title);
    assert_eq!(params["duration"], "223");
    for request in requests {
        assert!(request.headers["user-agent"].contains("github.com/amwps290/naviclient"));
        assert!(!request.headers.contains_key("authorization"));
        assert!(!request.headers.contains_key("cookie"));
        assert!(!request.headers.contains_key("referer"));
        assert!(request.body.is_empty());
    }
}

#[tokio::test]
async fn misses_invalid_json_and_cooldown_have_distinct_results() {
    let server = Server::new(vec![
        (404, "not found".into()),
        (200, "<html>bad json</html>".into()),
        (429, "<html>busy</html>".into()),
    ]);
    let client = Lrclib::test_client(server.url(""));
    let query = Query {
        title: "song".into(),
        ..Default::default()
    };
    assert_eq!(client.lookup(&query).await.unwrap(), None);
    assert_eq!(
        client.lookup(&query).await.unwrap_err(),
        ProviderError::InvalidResponse
    );
    assert_eq!(
        client.search(&query).await.unwrap_err(),
        ProviderError::RateLimited { retry_seconds: 30 }
    );
    assert!(matches!(
        client.search(&query).await,
        Err(ProviderError::RateLimited { .. })
    ));
    assert_eq!(server.requests().len(), 3);
}

#[tokio::test]
async fn sequential_requests_reserve_at_least_three_hundred_milliseconds() {
    let server = Server::new(vec![success(json!([])), success(json!([]))]);
    let client = Lrclib::test_client(server.url(""));
    let query = Query {
        title: "song".into(),
        ..Default::default()
    };
    client.search(&query).await.unwrap();
    let start = Instant::now();
    client.search(&query).await.unwrap();
    assert!(start.elapsed() >= Duration::from_millis(290));
}

#[test]
fn retry_after_accepts_seconds_and_http_dates() {
    let now = std::time::UNIX_EPOCH;
    assert_eq!(retry_after(Some("120"), now), Some(120));
    assert_eq!(
        retry_after(Some("Thu, 01 Jan 1970 00:01:00 GMT"), now),
        Some(60)
    );
    assert_eq!(retry_after(Some("broken"), now), None);
}

async fn client_for_response(response: String) -> (Lrclib, tokio::task::JoinHandle<()>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Lrclib::test_client(format!("http://{}", listener.local_addr().unwrap()));
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(stream.read_u8().await.unwrap());
        }
        stream.write_all(response.as_bytes()).await.unwrap();
    });
    (client, task)
}

#[tokio::test]
async fn retry_after_cooldown_survives_rebuilding_the_network_client() {
    let (client, server) = client_for_response(
        "HTTP/1.1 503 Busy\r\nRetry-After: 12\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .into(),
    )
    .await;
    let query = Query {
        title: "song".into(),
        ..Default::default()
    };
    assert_eq!(
        client.search(&query).await.unwrap_err(),
        ProviderError::RateLimited { retry_seconds: 12 }
    );
    server.await.unwrap();
    let mut rebuilt = Lrclib::new(&NetworkRoute::Direct, client.gate.clone()).unwrap();
    rebuilt.base = client.base;
    // The fixture has closed. A cooldown must reject this without another HTTP request.
    assert!(matches!(
        rebuilt.search(&query).await,
        Err(ProviderError::RateLimited {
            retry_seconds: 1..=13
        })
    ));
}

#[tokio::test]
async fn unavailable_without_retry_header_and_non_json_success_are_distinct_errors() {
    for (response, expected) in [
        ("HTTP/1.1 503 Busy\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", ProviderError::Unavailable),
        ("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]", ProviderError::InvalidResponse),
    ] {
        let (client, server) = client_for_response(response.into()).await;
        assert_eq!(client.search(&Query { title: "song".into(), ..Default::default() }).await.unwrap_err(), expected);
        server.await.unwrap();
    }
}

#[tokio::test]
async fn cancelling_an_http_request_allows_the_next_one_after_the_spacing_interval() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = Lrclib::test_client(format!("http://{}", listener.local_addr().unwrap()));
    let (sent, received) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut first, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(first.read_u8().await.unwrap());
        }
        sent.send(()).unwrap();
        let (mut second, _) = listener.accept().await.unwrap();
        request.clear();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(second.read_u8().await.unwrap());
        }
        second.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n[]").await.unwrap();
    });
    let query = Query {
        title: "song".into(),
        ..Default::default()
    };
    let pending_client = client.clone();
    let pending_query = query.clone();
    let pending = tokio::spawn(async move { pending_client.search(&pending_query).await });
    tokio::time::timeout(Duration::from_secs(2), received)
        .await
        .unwrap()
        .unwrap();
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    let start = Instant::now();
    assert!(
        tokio::time::timeout(Duration::from_secs(2), client.search(&query))
            .await
            .unwrap()
            .unwrap()
            .is_empty()
    );
    assert!(start.elapsed() >= Duration::from_millis(290));
    server.await.unwrap();
}
