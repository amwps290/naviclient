//! Scripted and routed GET/POST responses for local HTTP contract tests.

use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde_json::Value;
use url::Url;

const MAX_REQUEST_HEADER_BYTES: usize = 16 * 1024;

#[derive(Clone)]
pub(crate) struct Request {
    pub(crate) method: String,
    pub(crate) url: Url,
    pub(crate) headers: BTreeMap<String, String>,
    pub(crate) body: Vec<u8>,
}

pub(crate) struct Server {
    address: SocketAddr,
    requests: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Server {
    pub(crate) fn new(responses: Vec<(u16, String)>) -> Self {
        let mut responses = VecDeque::from(responses);
        Self::with_handler(move |_| {
            responses
                .pop_front()
                .unwrap_or((500, "unexpected request".into()))
        })
    }

    pub(crate) fn with_handler(
        mut respond: impl FnMut(&Request) -> (u16, String) + Send + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let worker = thread::spawn(move || loop {
            let (stream, _) = listener.accept().unwrap();
            if stopped.load(Ordering::SeqCst) {
                break;
            }
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut reader = BufReader::new(stream);
            let mut first_line = String::new();
            reader.read_line(&mut first_line).unwrap();
            let mut header_bytes = first_line.len();
            assert!(header_bytes <= MAX_REQUEST_HEADER_BYTES);
            let mut parts = first_line.split_whitespace();
            let method = parts.next().unwrap().to_string();
            let url = Url::parse(&format!("http://fixture{}", parts.next().unwrap())).unwrap();
            let mut headers = BTreeMap::new();
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                header_bytes += line.len();
                assert!(header_bytes <= MAX_REQUEST_HEADER_BYTES);
                if line == "\r\n" {
                    break;
                }
                let (name, value) = line.split_once(':').unwrap();
                headers.insert(name.to_ascii_lowercase(), value.trim().to_string());
            }
            let length = headers
                .get("content-length")
                .map_or(0, |value| value.parse::<usize>().unwrap());
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let request = Request {
                method,
                url,
                headers,
                body,
            };
            let (status, body) = respond(&request);
            recorded.lock().unwrap().push(request);
            write!(reader.get_mut(), "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        });
        Self {
            address,
            requests,
            stop,
            worker: Some(worker),
        }
    }

    pub(crate) fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    pub(crate) fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wake the blocking accept so fixtures join even after an early failure.
        let _ = TcpStream::connect(self.address);
        let result = self.worker.take().unwrap().join();
        // Preserve the original test failure if the handler also panicked.
        if !thread::panicking() {
            result.unwrap();
        }
    }
}

pub(crate) fn success(body: Value) -> (u16, String) {
    (200, body.to_string())
}
