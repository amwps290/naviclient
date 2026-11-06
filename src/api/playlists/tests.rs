use super::*;
use crate::test_support::http::{success, Request, Server as HttpServer};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct State {
    name: String,
    songs: Vec<String>,
    owner: String,
    exists: bool,
    fail_write: bool,
    writes: Vec<(String, Vec<(String, String)>)>,
}

struct Server {
    api: OpenSubsonicProvider,
    state: Arc<Mutex<State>>,
    _http: HttpServer,
}

impl Server {
    fn new() -> Self {
        let state = Arc::new(Mutex::new(State {
            owner: "alice".into(),
            ..Default::default()
        }));
        let worker_state = state.clone();
        let http = HttpServer::with_handler(move |request| respond(request, &worker_state));
        let api = OpenSubsonicProvider::new(&http.url(""), "alice", "secret").unwrap();
        Self {
            api,
            state,
            _http: http,
        }
    }
}

fn respond(request: &Request, state: &Mutex<State>) -> (u16, String) {
    let method = request.method.as_str();
    let url = &request.url;
    let target = &url[url::Position::BeforePath..];
    let endpoint = url
        .path()
        .trim_start_matches("/rest/")
        .trim_end_matches(".view");
    let mut params: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    let form: Vec<_> = url::form_urlencoded::parse(&request.body)
        .into_owned()
        .collect();
    params.extend(form.clone());
    let values = |key: &str| {
        params
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .collect::<Vec<_>>()
    };
    let mut state = state.lock().unwrap();
    let mut body = serde_json::json!({"status":"ok", "version":"1.16.1"});
    if endpoint != "getPlaylist" {
        assert_eq!(method, "POST");
        assert!(
            target.len() < 512,
            "playlist entries must be in the form body"
        );
        assert!(!form.is_empty());
        state.writes.push((endpoint.into(), form));
        if state.fail_write {
            body = serde_json::json!({"status":"failed", "error":{"code":50,"message":"Permission denied"}});
        } else {
            match endpoint {
                "createPlaylist" => {
                    if let Some(name) = values("name").first() {
                        state.name = name.clone();
                    }
                    state.songs = values("songId");
                    state.exists = true;
                }
                "updatePlaylist" => {
                    assert_eq!(values("playlistId"), ["p"]);
                    if let Some(name) = values("name").first() {
                        state.name = name.clone();
                    }
                    if let Some(index) = values("songIndexToRemove").first() {
                        state.songs.remove(index.parse::<usize>().unwrap());
                    }
                    state.songs.extend(values("songIdToAdd"));
                }
                "deletePlaylist" => {
                    assert_eq!(values("id"), ["p"]);
                    state.exists = false;
                }
                other => panic!("unexpected endpoint {other}"),
            }
        }
    }
    if body["status"] == "ok" && matches!(endpoint, "getPlaylist" | "createPlaylist") {
        body["playlist"] = serde_json::json!({
            "id":"p", "name":state.name, "owner":state.owner, "songCount":state.songs.len(),
            "entry": state.songs.iter().map(|id| serde_json::json!({"id":id,"title":id})).collect::<Vec<_>>()
        });
    }
    success(serde_json::json!({"subsonic-response":body}))
}

fn ids(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| v.to_string()).collect()
}

#[tokio::test]
async fn playlist_crud_preserves_duplicate_occurrences_and_unicode_names() {
    let server = Server::new();
    let api = &server.api;
    let created = api
        .edit_playlist(&PlaylistEdit::Create {
            name: " 初始歌单 ".into(),
            songs: ids(&["a", "b", "a"]),
        })
        .await
        .unwrap();
    assert_eq!(created.id, "p");
    assert!(!created.deleted);
    api.edit_playlist(&PlaylistEdit::Rename {
        id: "p".into(),
        name: "重命名 & +".into(),
    })
    .await
    .unwrap();
    api.edit_playlist(&PlaylistEdit::Append {
        id: "p".into(),
        songs: ids(&["a"]),
    })
    .await
    .unwrap();
    api.edit_playlist(&PlaylistEdit::Move {
        id: "p".into(),
        expected: ids(&["a", "b", "a", "a"]),
        from: 2,
        to: 1,
    })
    .await
    .unwrap();
    assert_eq!(
        song_ids(&api.playlist("p").await.unwrap().songs),
        ids(&["a", "a", "b", "a"])
    );
    api.edit_playlist(&PlaylistEdit::Remove {
        id: "p".into(),
        expected: ids(&["a", "a", "b", "a"]),
        index: 1,
    })
    .await
    .unwrap();
    let details = api.playlist("p").await.unwrap();
    assert_eq!(details.playlist.name, "重命名 & +");
    assert_eq!(song_ids(&details.songs), ids(&["a", "b", "a"]));
    assert!(
        api.edit_playlist(&PlaylistEdit::Delete { id: "p".into() })
            .await
            .unwrap()
            .deleted
    );
    assert!(!server.state.lock().unwrap().exists);
}

#[tokio::test]
async fn stale_entries_and_shared_ownership_never_send_a_write() {
    let server = Server::new();
    server.state.lock().unwrap().songs = ids(&["a", "b", "a"]);
    let edit = PlaylistEdit::Remove {
        id: "p".into(),
        expected: ids(&["a", "a", "b"]),
        index: 1,
    };
    assert!(server
        .api
        .edit_playlist(&edit)
        .await
        .unwrap_err()
        .to_string()
        .contains("changed"));
    server.state.lock().unwrap().owner = "bob".into();
    assert!(server
        .api
        .edit_playlist(&PlaylistEdit::Delete { id: "p".into() })
        .await
        .is_err());
    assert!(server.state.lock().unwrap().writes.is_empty());
}

#[tokio::test]
async fn server_rejection_is_reported_without_retry_or_local_changes() {
    let server = Server::new();
    server.state.lock().unwrap().fail_write = true;
    let result = server
        .api
        .edit_playlist(&PlaylistEdit::Append {
            id: "p".into(),
            songs: ids(&["new"]),
        })
        .await;
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Permission denied"));
    let state = server.state.lock().unwrap();
    assert_eq!(state.writes.len(), 1);
    assert!(state.songs.is_empty());
}

#[tokio::test]
async fn large_queues_use_repeated_form_parameters_and_empty_playlists_are_supported() {
    let server = Server::new();
    server
        .api
        .edit_playlist(&PlaylistEdit::Create {
            name: "Empty".into(),
            songs: vec![],
        })
        .await
        .unwrap();
    assert!(server.api.playlist("p").await.unwrap().songs.is_empty());
    let songs: Vec<_> = (0..2000).map(|i| format!("song {i}&")).collect();
    server
        .api
        .edit_playlist(&PlaylistEdit::Create {
            name: "Large".into(),
            songs: songs.clone(),
        })
        .await
        .unwrap();
    assert_eq!(server.state.lock().unwrap().songs, songs);
}
