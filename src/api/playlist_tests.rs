use super::*;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

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
    api: Api,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let api = Api::new(
            &format!("http://{}", listener.local_addr().unwrap()),
            "alice",
            "secret",
        )
        .unwrap();
        listener.set_nonblocking(true).unwrap();
        let state = Arc::new(Mutex::new(State {
            owner: "alice".into(),
            ..Default::default()
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_state = state.clone();
        let worker_stop = stop.clone();
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => respond(stream, &worker_state),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(error) => panic!("fixture listener: {error}"),
                }
            }
        });
        Self {
            api,
            state,
            stop,
            worker: Some(worker),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn respond(mut stream: TcpStream, state: &Mutex<State>) {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut bytes = Vec::new();
    let header_end = loop {
        let mut buffer = [0; 4096];
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0);
        bytes.extend_from_slice(&buffer[..read]);
        if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let header = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
    let length = header
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    while bytes.len() < header_end + length {
        let mut buffer = [0; 4096];
        let read = stream.read(&mut buffer).unwrap();
        assert!(read > 0);
        bytes.extend_from_slice(&buffer[..read]);
    }
    let mut request = header.lines().next().unwrap().split_whitespace();
    let method = request.next().unwrap();
    let target = request.next().unwrap();
    let url = Url::parse(&format!("http://localhost{target}")).unwrap();
    let endpoint = url
        .path()
        .trim_start_matches("/rest/")
        .trim_end_matches(".view");
    let mut params: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    let form: Vec<_> = url::form_urlencoded::parse(&bytes[header_end..])
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
    let response = serde_json::json!({"subsonic-response":body}).to_string();
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response).unwrap();
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
