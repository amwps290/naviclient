use super::super::album_plan::merge_album_plan;
use super::super::style::select_style_seeds;
use super::*;
use crate::models::Album;
use crate::recommendation::ListeningProfile;
use crate::test_support::http::{success, Request, Server};
use std::{
    collections::{BTreeSet, HashMap},
    time::Duration,
};

fn api(server: &Server) -> OpenSubsonicProvider {
    OpenSubsonicProvider::new(&server.url(""), "listener", "fixture").unwrap()
}

fn respond(request: &Request) -> (u16, String) {
    let url = &request.url;
    let params: HashMap<_, _> = url.query_pairs().into_owned().collect();
    let mut body = serde_json::json!({"status":"ok", "version":"1.16.1"});
    match url.path() {
        "/rest/getAlbum.view" => {
            let id = &params["id"];
            if id == "bad" {
                body = serde_json::json!({"status":"failed", "error":{"code":70, "message":"Album unavailable"}});
            } else {
                body["album"] = serde_json::json!({"song": (0..14).map(|i| {
                    serde_json::json!({"id":format!("{id}-{i}"), "title":"Track"})
                }).collect::<Vec<_>>()});
            }
        }
        "/rest/getSimilarSongs2.view" => {
            body["similarSongs2"] = serde_json::json!({"song":[{"id":format!("similar-{}", params["id"]), "title":"Similar"}]});
        }
        "/rest/getSongsByGenre.view" => {
            body["songsByGenre"] = serde_json::json!({"song":[{"id":format!("genre-{}", params["genre"]), "title":"Genre"}]});
        }
        path => panic!("unexpected collection request: {path}"),
    }
    success(serde_json::json!({"subsonic-response":body}))
}

fn album(id: &str) -> Album {
    Album {
        id: id.into(),
        ..Default::default()
    }
}

#[tokio::test]
async fn album_requests_deduplicate_sources_cap_tracks_and_keep_successes_after_failure() {
    let server = Server::with_handler(respond);
    let plan = merge_album_plan([
        (
            vec![album("bad"), album("shared"), album("second")],
            CandidateSource::Frequent,
        ),
        (vec![album("shared")], CandidateSource::Newest),
    ]);
    let candidates = tokio::time::timeout(
        Duration::from_secs(5),
        collect_album_candidates(api(&server), plan),
    )
    .await
    .unwrap();
    assert_eq!(candidates.len(), 36);
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| &candidate.song.id)
            .collect::<BTreeSet<_>>()
            .len(),
        24
    );
    for i in 0..12 {
        assert_eq!(
            candidates
                .iter()
                .filter(|candidate| candidate.song.id == format!("shared-{i}"))
                .map(|candidate| candidate.source)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([CandidateSource::Frequent, CandidateSource::Newest])
        );
        assert!(candidates
            .iter()
            .any(|candidate| candidate.song.id == format!("second-{i}")
                && candidate.source == CandidateSource::Frequent));
    }
    let requests = server.requests();
    assert_eq!(requests.len(), 3);
    assert!(requests
        .iter()
        .all(|request| request.url.path() == "/rest/getAlbum.view"));
    let ids = requests
        .iter()
        .map(|request| {
            request
                .url
                .query_pairs()
                .find(|(key, _)| key == "id")
                .unwrap()
                .1
                .into_owned()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        ids,
        BTreeSet::from(["bad".into(), "shared".into(), "second".into()])
    );
}

#[tokio::test]
async fn seed_requests_preserve_daily_limits_counts_and_genre_spelling() {
    let server = Server::with_handler(respond);
    let api = api(&server);
    let favorites = (0..7)
        .map(|i| Song {
            id: format!("seed-{i}"),
            artist: "Artist".into(),
            genre: Some("Ambient".into()),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    let seeds = select_style_seeds(&favorites, &[], &ListeningProfile::default());
    let similar = tokio::time::timeout(
        Duration::from_secs(5),
        collect_similar_candidates(api.clone(), &seeds),
    )
    .await
    .unwrap();
    assert_eq!(similar.len(), 4);
    assert!(similar
        .iter()
        .all(|candidate| candidate.source == CandidateSource::Similar));
    let seeds = [" Ambient ", "ambient", "Jazz & Soul", "Metal"]
        .into_iter()
        .map(|genre| Song {
            genre: Some(genre.into()),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    let genres = tokio::time::timeout(
        Duration::from_secs(5),
        collect_genre_candidates(api, &seeds),
    )
    .await
    .unwrap();
    assert_eq!(genres.len(), 2);
    assert!(genres
        .iter()
        .all(|candidate| candidate.source == CandidateSource::SameGenre));
    let requests = server.requests();
    assert_eq!(requests.len(), 6);
    let mut genre_names = BTreeSet::new();
    let mut similar_ids = BTreeSet::new();
    for request in requests.iter() {
        let params: HashMap<_, _> = request.url.query_pairs().into_owned().collect();
        assert_eq!(params["count"], "20");
        if request.url.path() == "/rest/getSongsByGenre.view" {
            assert_eq!(params["offset"], "0");
            genre_names.insert(params["genre"].clone());
        } else {
            assert_eq!(request.url.path(), "/rest/getSimilarSongs2.view");
            similar_ids.insert(params["id"].clone());
        }
    }
    assert_eq!(
        genre_names,
        BTreeSet::from(["Ambient".into(), "Jazz & Soul".into()])
    );
    assert_eq!(similar_ids, (0..4).map(|i| format!("seed-{i}")).collect());
}
