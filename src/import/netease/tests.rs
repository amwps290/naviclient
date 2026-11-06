use super::*;
use serde_json::json;
use std::collections::BTreeMap;

use crate::test_support::http::{success, Server};

impl Server {
    async fn fetch_netease(&self) -> Result<ImportedPlaylist> {
        fetch_from(
            &super::super::http_client(),
            "42",
            &self.url("/api/v6/playlist/detail"),
            &self.url("/api/song/detail"),
        )
        .await
    }
}

fn song(id: u64) -> Value {
    json!({"id": id, "name": format!("Track {id}"), "ar": [{"name": "Artist"}], "dt": 123_000})
}

fn playlist(count: u64, embedded: Vec<Value>) -> Value {
    json!({
        "code": 200,
        "playlist": {
            "name": "Fixture",
            "trackCount": count,
            "trackIds": (1..=count).map(|id| json!({"id": id})).collect::<Vec<_>>(),
            "tracks": embedded
        }
    })
}

#[tokio::test]
async fn large_playlist_requests_bounded_batches_then_one_missing_pass_and_restores_order() {
    let missing = [1, 502, 1001];
    let mut responses = vec![success(playlist(
        1001,
        vec![json!({"id": 2, "name": "stale"})],
    ))];
    for range in [1..=500, 501..=1000, 1001..=1001] {
        let songs: Vec<_> = range
            .rev()
            .filter(|id| !missing.contains(id))
            .map(song)
            .collect();
        responses.push(success(json!({"code": 200, "songs": songs})));
    }
    responses.push(success(
        json!({"code": "200", "songs": [song(1001), song(1)]}),
    ));
    let server = Server::new(responses);
    let imported = server.fetch_netease().await.unwrap();
    assert_eq!(imported.name, "Fixture");
    assert_eq!(imported.unavailable_tracks, 1);
    assert_eq!(
        imported
            .tracks
            .iter()
            .map(|track| track.title.clone())
            .collect::<Vec<_>>(),
        (1..=1001)
            .filter(|id| *id != 502)
            .map(|id| format!("Track {id}"))
            .collect::<Vec<_>>()
    );
    assert!(imported
        .tracks
        .iter()
        .all(|track| track.duration_ms == 123_000));

    let requests = server.requests();
    assert_eq!(requests.len(), 5);
    assert_eq!(requests[0].url.path(), "/api/v6/playlist/detail");
    assert_eq!(
        requests[0]
            .url
            .query_pairs()
            .into_owned()
            .collect::<BTreeMap<_, _>>(),
        BTreeMap::from([
            ("id".into(), "42".into()),
            ("n".into(), "100000".into()),
            ("s".into(), "0".into())
        ])
    );
    let batches: Vec<Vec<u64>> = requests
        .iter()
        .skip(1)
        .map(|request| {
            assert_eq!(request.url.path(), "/api/song/detail");
            let params: Vec<_> = request.url.query_pairs().collect();
            assert_eq!(params.len(), 1);
            assert_eq!(params[0].0, "ids");
            serde_json::from_str(&params[0].1).unwrap()
        })
        .collect();
    assert_eq!(
        batches,
        vec![
            (1..=500).collect(),
            (501..=1000).collect(),
            vec![1001],
            vec![1, 502, 1001]
        ]
    );
    for request in requests.iter() {
        assert_eq!(request.method, "GET");
        assert!(request.body.is_empty());
        assert_eq!(request.headers["referer"], REFERER);
        assert_eq!(request.headers["user-agent"], super::super::BROWSER_UA);
    }
}

#[tokio::test]
async fn complete_embedded_tracks_skip_detail_requests_while_declared_count_is_still_validated() {
    for count in [2, 3] {
        let mut body = playlist(2, vec![song(1), song(2)]);
        body["playlist"]["trackCount"] = json!(count);
        let server = Server::new(vec![success(body)]);
        let result = server.fetch_netease().await;
        if count == 2 {
            assert_eq!(result.unwrap().tracks.len(), 2);
        } else {
            assert!(result.unwrap_err().to_string().contains("2/3"));
        }
        assert_eq!(server.requests().len(), 1);
    }
}

#[tokio::test]
async fn request_errors_abort_playlist_fetch_and_both_detail_passes() {
    let initial = success(playlist(2, vec![]));
    let one_song = success(json!({"code": 200, "songs": [song(1)]}));
    let http_error = (503, "unavailable".into());
    let json_error = (200, "invalid JSON".into());
    let api_error = success(json!({"code": "460"}));
    for (responses, expected) in [
        (vec![http_error.clone()], Some("HTTP 503")),
        (vec![json_error.clone()], None),
        (vec![api_error.clone()], Some("API 460")),
        (vec![initial.clone(), http_error.clone()], Some("HTTP 503")),
        (vec![initial.clone(), json_error], None),
        (vec![initial.clone(), api_error], Some("API 460")),
        (vec![initial, one_song, http_error], Some("HTTP 503")),
    ] {
        let count = responses.len();
        let server = Server::new(responses);
        let error = server.fetch_netease().await.unwrap_err();
        if let Some(expected) = expected {
            assert!(error.to_string().contains(expected));
        }
        assert_eq!(server.requests().len(), count);
    }
}
