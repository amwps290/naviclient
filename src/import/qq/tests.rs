use super::*;
use crate::test_support::http::{success, Server};
use serde_json::json;

impl Server {
    async fn fetch_qq(&self, id: &str) -> Result<ImportedPlaylist> {
        fetch_from(
            &super::super::http_client(),
            id,
            &self.url("/cgi-bin/musicu.fcg"),
        )
        .await
    }
}

fn song(name: &str) -> Value {
    json!({
        "name": name,
        "singer": [{"name": "Artist"}],
        "album": {"name": "Album"},
        "interval": "269"
    })
}

fn response(tracks: Vec<Value>, count: usize) -> Value {
    json!({
        "code": 0,
        "req": {"code": 0, "data": {
            "code": 0,
            "subcode": 0,
            "dirinfo": {"title": "  Fixture  ", "songnum": count},
            "songlist": tracks
        }}
    })
}

#[tokio::test]
async fn musicu_post_keeps_payload_headers_numeric_id_and_ordered_duplicate_tracks() {
    let server = Server::new(vec![success(response(
        vec![song("First"), song("Second"), song("First")],
        5,
    ))]);
    let imported = server.fetch_qq("00042").await.unwrap();
    assert_eq!(imported.name, "Fixture");
    assert_eq!(imported.unavailable_tracks, 2);
    assert_eq!(
        imported
            .tracks
            .iter()
            .map(|track| track.title.as_str())
            .collect::<Vec<_>>(),
        ["First", "Second", "First"]
    );
    assert!(imported
        .tracks
        .iter()
        .all(|track| track.duration_ms == 269_000
            && track.artists == ["Artist"]
            && track.album == "Album"));

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.method, "POST");
    assert_eq!(request.url.path(), "/cgi-bin/musicu.fcg");
    assert!(request.url.query().is_none());
    assert_eq!(request.headers["referer"], REFERER);
    assert_eq!(request.headers["user-agent"], super::super::BROWSER_UA);
    assert_eq!(request.headers["content-type"], "application/json");
    assert!(!request.headers.contains_key("cookie"));
    assert!(!request.headers.contains_key("authorization"));
    let payload: Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(
        payload,
        json!({
            "comm": {"ct": 24, "cv": 0},
            "req": {
                "module": "music.srfDissInfo.aiDissInfo",
                "method": "uniform_get_Dissinfo",
                "param": {
                    "disstid": 42,
                    "enc_host_uin": "",
                    "tag": 1,
                    "userinfo": 1,
                    "song_begin": 0,
                    "song_num": 10000
                }
            }
        })
    );
}

#[tokio::test]
async fn each_layered_api_error_is_rejected_and_omitted_status_codes_remain_compatible() {
    for path in ["/code", "/req/code", "/req/data/code", "/req/data/subcode"] {
        let mut body = response(vec![song("First")], 1);
        *body.pointer_mut(path).unwrap() = json!("460");
        let server = Server::new(vec![success(body)]);
        let error = server.fetch_qq("42").await.unwrap_err();
        assert!(error.to_string().contains(if path.ends_with("subcode") {
            "API subcode 460"
        } else {
            "API code 460"
        }));
        assert_eq!(server.requests().len(), 1);
    }
    let mut body = response(vec![song("First")], 1);
    for (path, fields) in [
        ("", vec!["code"]),
        ("/req", vec!["code"]),
        ("/req/data", vec!["code", "subcode"]),
    ] {
        let object = body.pointer_mut(path).unwrap().as_object_mut().unwrap();
        for field in fields {
            object.remove(field);
        }
    }
    let server = Server::new(vec![success(body)]);
    assert_eq!(server.fetch_qq("42").await.unwrap().tracks.len(), 1);
}

#[tokio::test]
async fn http_and_json_failures_stop_before_metadata_decoding() {
    for (status, body, expected) in [
        (
            503,
            response(vec![song("First")], 1).to_string(),
            Some("HTTP 503"),
        ),
        (200, "invalid JSON".into(), None),
    ] {
        let server = Server::new(vec![(status, body)]);
        let error = server.fetch_qq("42").await.unwrap_err();
        if let Some(expected) = expected {
            assert!(error.to_string().contains(expected));
        }
        assert_eq!(server.requests().len(), 1);
    }
}

#[tokio::test]
async fn unavailable_counts_do_not_hide_malformed_rows_in_successful_http_responses() {
    for malformed in [false, true] {
        let tracks = if malformed {
            vec![song("First"), json!({"name": "Missing singer"})]
        } else {
            vec![song("First")]
        };
        let server = Server::new(vec![success(response(tracks, 2))]);
        let result = server.fetch_qq("42").await;
        if malformed {
            assert!(result.unwrap_err().to_string().contains("1/2"));
        } else {
            let imported = result.unwrap();
            assert_eq!(imported.tracks.len(), 1);
            assert_eq!(imported.unavailable_tracks, 1);
        }
    }
}

#[tokio::test]
async fn invalid_playlist_ids_are_rejected_before_any_http_request() {
    for id in ["not-a-number", "-1", "18446744073709551616"] {
        let server = Server::new(vec![]);
        assert!(server.fetch_qq(id).await.is_err());
        assert!(server.requests().is_empty());
    }
}
