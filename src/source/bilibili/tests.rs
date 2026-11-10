use super::*;
use serde_json::json;
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};

pub(super) fn session() -> AuthSession {
    AuthSession::decode_secret(
        zeroize::Zeroizing::new(b"SESSDATA=fake; bili_jct=csrf; DedeUserID=123".to_vec()),
        123,
    )
    .unwrap()
}

pub(super) struct Fixture {
    origin: String,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
    pub(super) paths: Arc<Mutex<Vec<String>>>,
    pub(super) headers: Arc<Mutex<Vec<BTreeMap<String, String>>>>,
}
impl Fixture {
    pub(super) fn new(respond: impl Fn(&str) -> serde_json::Value + Send + Sync + 'static) -> Self {
        Self::with_response(move |path, _| {
            let body = respond(path).to_string();
            format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes()
        })
    }

    pub(super) fn with_response(
        respond: impl Fn(&str, &BTreeMap<String, String>) -> Vec<u8> + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let paths = Arc::new(Mutex::new(Vec::new()));
        let recorded = paths.clone();
        let headers = Arc::new(Mutex::new(Vec::new()));
        let recorded_headers = headers.clone();
        let respond = Arc::new(respond);
        let worker = thread::spawn(move || {
            let mut jobs = Vec::new();
            loop {
                let (stream, _) = listener.accept().unwrap();
                if stopped.load(Ordering::SeqCst) {
                    break;
                }
                let respond = respond.clone();
                let recorded = recorded.clone();
                let recorded_headers = recorded_headers.clone();
                jobs.push(thread::spawn(move || {
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                        .unwrap();
                    let mut reader = BufReader::new(stream);
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    let path = line.split_whitespace().nth(1).unwrap().to_string();
                    let mut headers = BTreeMap::new();
                    let mut size = line.len();
                    loop {
                        line.clear();
                        assert!(reader.read_line(&mut line).unwrap() > 0);
                        if line == "\r\n" {
                            break;
                        }
                        size += line.len();
                        assert!(size < 16 * 1024);
                        let (key, value) = line.split_once(':').unwrap();
                        headers.insert(key.to_ascii_lowercase(), value.trim().to_string());
                    }
                    recorded.lock().unwrap().push(path.clone());
                    recorded_headers.lock().unwrap().push(headers.clone());
                    let response = respond(&path, &headers);
                    let _ = reader.get_mut().write_all(&response);
                }));
            }
            for job in jobs {
                job.join().unwrap();
            }
        });
        Self {
            origin,
            stop,
            worker: Some(worker),
            paths,
            headers,
        }
    }
    pub(super) fn provider(&self) -> BilibiliProvider {
        BilibiliProvider {
            id: SourceId::new(ProviderKind::Bilibili, "test"),
            client: BilibiliClient::local(self.origin.clone()),
            session: session(),
            route: crate::models::network::NetworkRoute::Direct,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.origin.strip_prefix("http://").unwrap());
        self.worker.take().unwrap().join().unwrap();
    }
}

fn folders() -> serde_json::Value {
    json!({"code":0,"data":{"list":[{"id":5,"title":"Music","media_count":3}]}})
}
fn page(owner: u64, more: bool, items: serde_json::Value) -> serde_json::Value {
    json!({"code":0,"data":{"info":{"id":5,"title":"Music","media_count":3,"upper":{"mid":owner}},"has_more":more,"medias":items}})
}
pub(super) fn video(bvid: &str, parts: &[u64]) -> serde_json::Value {
    json!({"code":0,"data":{"bvid":bvid,"title":"Video title","pic":"http://i0.hdslb.com/bfs/archive/a.jpg","owner":{"mid":1,"name":"Uploader"},"pages":parts.iter().enumerate().map(|(i,cid)|json!({"cid":cid,"page":i+1,"part":format!("part {}", i+1),"duration":100+i})).collect::<Vec<_>>()}})
}

#[tokio::test]
async fn all_pages_and_parts_keep_order_and_skip_deleted_videos() {
    let fixture = Fixture::new(|path| {
        if path.contains("list-all") {
            folders()
        } else if path.contains("resource/list") {
            if path.contains("pn=1") {
                page(
                    123,
                    true,
                    json!([{"type":2,"bvid":"BV1GJ411x7h7"},{"type":2,"bvid":"BV1GJ411x7h8"}]),
                )
            } else {
                page(
                    123,
                    false,
                    json!([{"type":2,"bvid":"BV1GJ411x7h7"},{"type":2,"bvid":"BV1GJ411x7h9"}]),
                )
            }
        } else if path.contains("BV1GJ411x7h7") {
            thread::sleep(std::time::Duration::from_millis(40));
            video("BV1GJ411x7h7", &[10, 11])
        } else if path.contains("BV1GJ411x7h8") {
            video("BV1GJ411x7h8", &[20])
        } else {
            json!({"code":-404,"data":null})
        }
    });
    let tracks = fixture.provider().favorite_tracks(5).await.unwrap();
    assert_eq!((tracks.pages, tracks.videos, tracks.unavailable), (2, 3, 1));
    assert_eq!(
        tracks
            .songs
            .iter()
            .map(|song| song.id.as_str())
            .collect::<Vec<_>>(),
        ["BV1GJ411x7h7:10", "BV1GJ411x7h7:11", "BV1GJ411x7h8:20"]
    );
    assert!(tracks.songs.iter().all(|song| song.title == "Video title"
        && song.artist.is_empty()
        && song.album.is_empty()));
    assert_eq!(tracks.songs[1].duration, Some(101));
    assert_eq!(tracks.songs[1].bilibili_part.as_ref().unwrap().index, 2);
    assert_eq!(
        tracks.songs[0].cover_art.as_deref(),
        Some("https://i0.hdslb.com/bfs/archive/a.jpg")
    );
}

#[tokio::test]
async fn foreign_folder_and_owner_are_rejected_before_details() {
    let fixture = Fixture::new(|path| {
        if path.contains("list-all") {
            folders()
        } else {
            page(456, false, json!([]))
        }
    });
    assert_eq!(
        fixture.provider().favorite_tracks(6).await.unwrap_err(),
        BilibiliError::PermissionDenied
    );
    assert_eq!(
        fixture.provider().favorite_tracks(5).await.unwrap_err(),
        BilibiliError::AccountMismatch
    );
    assert!(!fixture
        .paths
        .lock()
        .unwrap()
        .iter()
        .any(|path| path.contains("/view")));
}

#[tokio::test]
async fn transient_failure_cannot_create_partial_play_all() {
    let fixture = Fixture::new(|path| {
        if path.contains("list-all") {
            folders()
        } else if path.contains("resource/list") {
            page(123, false, json!([{"type":2,"bvid":"BV1GJ411x7h7"}]))
        } else {
            json!({"code":-412,"data":null})
        }
    });
    assert_eq!(
        fixture.provider().favorite_tracks(5).await.unwrap_err(),
        BilibiliError::RateLimited
    );
}

#[test]
fn media_and_artwork_cannot_smuggle_addresses() {
    assert!(library::parse_media_id("BV1GJ411x7h7:1").is_ok());
    for id in [
        "BV1GJ411x7h7:01",
        "BV1GJ411x7h7:0",
        "BV1GJ411x7h7:1:2",
        "https://example.com:1",
    ] {
        assert!(library::parse_media_id(id).is_err());
    }
    for url in [
        "https://hdslb.com.evil.test/a.jpg",
        "https://user@i0.hdslb.com/a",
        "https://i0.hdslb.com:8443/a",
        "https://i0.hdslb.com/a?secret=x",
    ] {
        assert!(library::safe_artwork(url).is_none());
    }
}

#[tokio::test]
async fn empty_folder_completes_without_requesting_video_details() {
    let fixture = Fixture::new(|path| {
        if path.contains("list-all") {
            json!({"code":0,"data":{"list":[{"id":5,"title":"Empty","media_count":0}]}})
        } else {
            assert!(path.contains("resource/list"));
            json!({"code":0,"data":{"info":{"id":5,"title":"Empty","media_count":0,"upper":{"mid":123}},"has_more":false,"medias":null}})
        }
    });
    let tracks = fixture.provider().favorite_tracks(5).await.unwrap();
    assert!(tracks.songs.is_empty());
    assert_eq!((tracks.pages, tracks.videos, tracks.unavailable), (1, 0, 0));
    assert_eq!(fixture.paths.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn repeated_page_cannot_loop_or_publish_a_partial_folder() {
    let fixture = Fixture::new(|path| {
        if path.contains("list-all") {
            folders()
        } else {
            assert!(path.contains("resource/list"));
            page(123, true, json!([{"type":2,"bvid":"BV1GJ411x7h7"}]))
        }
    });
    assert_eq!(
        fixture.provider().favorite_tracks(5).await.unwrap_err(),
        BilibiliError::InvalidResponse
    );
    let paths = fixture.paths.lock().unwrap();
    assert_eq!(paths.len(), 3);
    assert!(paths.last().unwrap().contains("pn=2"));
}
