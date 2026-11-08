use super::*;
use crate::audio::preload::tests::{client, TestCache};
use crate::source::CachePolicy;
use reqwest::header::{HeaderMap, HeaderValue, REFERER, USER_AGENT};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Binary HTTP fixture with captured headers and bounded read/write timeouts.
pub(in crate::audio) struct Fixture {
    address: std::net::SocketAddr,
    requests: Arc<Mutex<Vec<BTreeMap<String, String>>>>,
    stopped: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Fixture {
    pub(in crate::audio) fn new(
        mut respond: impl FnMut(&str, &BTreeMap<String, String>) -> Vec<u8> + Send + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stop = stopped.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let worker = thread::spawn(move || {
            let mut responses = Vec::new();
            loop {
                let (stream, _) = listener.accept().unwrap();
                if worker_stop.load(Ordering::SeqCst) {
                    break;
                }
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(stream);
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let mut headers = BTreeMap::new();
                let mut count = first.len();
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    count += line.len();
                    assert!(count < 16 * 1024);
                    if line == "\r\n" {
                        break;
                    }
                    let (name, value) = line.split_once(':').unwrap();
                    headers.insert(name.to_ascii_lowercase(), value.trim().to_string());
                }
                recorded.lock().unwrap().push(headers.clone());
                let response = respond(first.split_whitespace().nth(1).unwrap(), &headers);
                // A decoder may abandon a response when it seeks to another range.
                // Real CDNs can serve the new range while the previous response
                // is being abandoned. Avoid blocking accept behind a large body.
                responses.push(thread::spawn(move || {
                    let _ = reader.get_mut().write_all(&response);
                }));
            }
            for response in responses {
                response.join().unwrap();
            }
        });
        Self {
            address,
            requests,
            stopped,
            worker: Some(worker),
        }
    }

    pub(in crate::audio) fn audio(bytes: Vec<u8>) -> Self {
        Self::new(move |_, headers| {
            let start: usize = headers["range"]
                .strip_prefix("bytes=")
                .unwrap()
                .strip_suffix('-')
                .unwrap()
                .parse()
                .unwrap();
            if start >= bytes.len() {
                return b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec();
            }
            let mut response = format!("HTTP/1.1 206 Partial Content\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nContent-Range: bytes {start}-{}/{}\r\nConnection: close\r\n\r\n", bytes.len() - start, bytes.len()-1, bytes.len()).into_bytes();
            response.extend_from_slice(&bytes[start..]);
            response
        })
    }

    pub(in crate::audio) fn url(&self) -> String {
        format!("http://{}/audio", self.address)
    }

    pub(in crate::audio) fn requests(&self) -> Vec<BTreeMap<String, String>> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.address);
        let result = self.worker.take().unwrap().join();
        if !thread::panicking() {
            result.unwrap();
        }
    }
}

pub(in crate::audio) fn resource(url: String) -> HttpResource {
    let mut headers = HeaderMap::new();
    headers.insert(
        REFERER,
        HeaderValue::from_static("https://www.bilibili.com/"),
    );
    headers.insert(USER_AGENT, HeaderValue::from_static("naviclient-fixture"));
    let mut http = HttpResource::new(url).with_headers(headers);
    http = http.with_cache_policy(CachePolicy::Temporary {
        max_bytes: 1024 * 1024,
    });
    http
}

#[test]
fn resource_headers_survive_ranges_and_do_not_enter_the_shared_client() {
    let fixture = Fixture::audio(b"audio".to_vec());
    let cache = TestCache::new();
    let shared = client();
    let mut http = resource(fixture.url());
    http = http.with_mime_hint(Some("audio/mp4".into()));
    let mut reader = HttpRangeReader::open_resource(
        shared.clone(),
        http,
        &cache.0,
        "one",
        StreamControl::new(false),
    )
    .unwrap();
    assert_eq!(reader.mime_type(), Some("audio/mp4"));
    let mut bytes = [0; 2];
    reader.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"au");
    reader.seek(SeekFrom::Start(4)).unwrap();
    reader.read_exact(&mut bytes[..1]).unwrap();
    assert_eq!(bytes[0], b'o');
    let mut other = HttpRangeReader::open_resource(
        shared,
        HttpResource::new(fixture.url()),
        &cache.0,
        "two",
        StreamControl::new(false),
    )
    .unwrap();
    other.read_exact(&mut bytes).unwrap();
    let requests = fixture.requests();
    assert_eq!(requests.len(), 3);
    for request in &requests[..2] {
        assert_eq!(request["referer"], "https://www.bilibili.com/");
        assert_eq!(request["user-agent"], "naviclient-fixture");
        assert!(!request.contains_key("cookie"));
    }
    assert_eq!(requests[1]["range"], "bytes=4-");
    assert!(!requests[2].contains_key("referer"));
    assert!(!requests[2].contains_key("user-agent"));
}

#[test]
fn resource_redirect_cannot_deliver_headers_to_another_origin() {
    let destination = Fixture::audio(b"audio".to_vec());
    let target = destination.url();
    let redirect = Fixture::new(move |_, _| {
        format!("HTTP/1.1 302 Found\r\nLocation: {target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").into_bytes()
    });
    let cache = TestCache::new();
    let error = match HttpRangeReader::open_resource(
        client(),
        resource(redirect.url()),
        &cache.0,
        "redirect",
        StreamControl::new(false),
    ) {
        Ok(_) => panic!("followed a redirect to another origin"),
        Err(error) => error,
    };
    assert_eq!(
        super::super::error::classify_anyhow(&error),
        StreamFailure::Http(302)
    );
    assert!(destination.requests().is_empty());
}

#[test]
fn temporary_cache_limit_does_not_prevent_reading_or_seeking_beyond_it() {
    let fixture = Fixture::audio(b"audio".to_vec());
    let cache = TestCache::new();
    let mut http = resource(fixture.url());
    http = http.with_cache_policy(CachePolicy::Temporary { max_bytes: 2 });
    let mut reader = HttpRangeReader::open_resource(
        client(),
        http,
        &cache.0,
        "temporary",
        StreamControl::new(false),
    )
    .unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"audio");
    assert_eq!(reader.buffer_progress().cached_bytes(), 2);
    reader.seek(SeekFrom::Start(3)).unwrap();
    bytes.clear();
    reader.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"io");
    drop(reader);
    assert_eq!(std::fs::read_dir(&cache.0).unwrap().count(), 0);
}

#[test]
fn resource_retry_preserves_headers() {
    let mut attempts = 0;
    let fixture = Fixture::new(move |_, headers| {
        attempts += 1;
        if attempts == 1 {
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 0-4/5\r\nConnection: close\r\n\r\nau".to_vec()
        } else {
            assert_eq!(headers["range"], "bytes=2-");
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 2-4/5\r\nConnection: close\r\n\r\ndio".to_vec()
        }
    });
    let cache = TestCache::new();
    let mut reader = HttpRangeReader::open_resource(
        client(),
        resource(fixture.url()),
        &cache.0,
        "retry",
        StreamControl::new(false),
    )
    .unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"audio");
    let requests = fixture.requests();
    assert_eq!(requests.len(), 2);
    for request in requests {
        assert_eq!(request["referer"], "https://www.bilibili.com/");
    }
}

#[test]
fn error_response_does_not_echo_credentials() {
    let fixture = Fixture::new(|_, _| {
        b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 17\r\nConnection: close\r\n\r\nCookie=raw-secret".to_vec()
    });
    let cache = TestCache::new();
    let error = match HttpRangeReader::open_resource(
        client(),
        resource(fixture.url()),
        &cache.0,
        "error",
        StreamControl::new(false),
    ) {
        Ok(_) => panic!("accepted an error envelope"),
        Err(error) => error,
    };
    assert!(!format!("{error:#}").contains("raw-secret"));
}

#[test]
fn same_origin_redirects_preserve_range_and_resource_headers() {
    let fixture = Fixture::new(|path, headers| {
        if path == "/audio" {
            b"HTTP/1.1 302 Found\r\nLocation: /next\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
        } else {
            assert_eq!(headers["range"], "bytes=0-");
            b"HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 0-4/5\r\nConnection: close\r\n\r\naudio".to_vec()
        }
    });
    let cache = TestCache::new();
    let mut reader = HttpRangeReader::open_resource(
        client(),
        resource(fixture.url()),
        &cache.0,
        "redirect",
        StreamControl::new(false),
    )
    .unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"audio");
    let requests = fixture.requests();
    assert_eq!(requests.len(), 2);
    for request in requests {
        assert_eq!(request["referer"], "https://www.bilibili.com/");
    }
}
