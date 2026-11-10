use super::*;
use reqwest::header::{HeaderMap, HeaderValue};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

// A server that accepts one connection and requires both requests to use it.
// Isolated caches keep this test independent of eviction in parallel tests.
fn keep_alive_server() -> (String, thread::JoinHandle<()>) {
    keep_alive_connections(1)
}

fn keep_alive_connections(connections: usize) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        for _ in 0..connections {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut reader = BufReader::new(stream);
            for expected in ["one", "two"] {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                assert!(line.contains(&format!("/audio?signature={expected}")));
                let mut found = false;
                loop {
                    line.clear();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                    let (name, value) = line.split_once(':').unwrap();
                    assert!(!name.eq_ignore_ascii_case("cookie"));
                    if name.eq_ignore_ascii_case("x-fixture") {
                        assert_eq!(value.trim(), expected);
                        found = true;
                    }
                }
                assert!(found);
                reader
                    .get_mut()
                    .write_all(
                        b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nConnection: keep-alive\r\n\r\na",
                    )
                    .unwrap();
            }
        }
    });
    (origin, worker)
}

fn resource(origin: &str, value: &str) -> HttpResource {
    let mut headers = HeaderMap::new();
    headers.insert("x-fixture", HeaderValue::from_str(value).unwrap());
    HttpResource::new(format!("{origin}/audio?signature={value}")).with_headers(headers)
}

#[test]
fn header_bound_resources_cannot_reuse_a_public_redirect_client() {
    let cache = ClientCache::<usize>::default();
    let public = HttpResource::new("https://fixture.test/audio");
    let bound = resource("https://fixture.test", "one");
    assert_eq!(cached(&cache, &public, |_| Ok(1)).unwrap(), 1);
    assert_eq!(cached(&cache, &bound, |_| Ok(2)).unwrap(), 2);
    assert_eq!(cache.lock().unwrap().len(), 2);
}

#[test]
fn blocking_pool_reuses_connections_across_signed_urls_and_request_headers() {
    let (origin, worker) = keep_alive_server();
    let cache = ClientCache::default();
    for value in ["one", "two"] {
        let resource = resource(&origin, value);
        let client = blocking_cached(&resource, &cache).unwrap();
        let bytes = client
            .get(resource.url())
            .headers(resource.headers().clone())
            .send()
            .unwrap()
            .bytes()
            .unwrap();
        assert_eq!(&bytes[..], b"a");
    }
    worker.join().unwrap();
}

#[tokio::test]
async fn probe_pool_reuses_connections_across_signed_urls_and_request_headers() {
    let (origin, worker) = keep_alive_server();
    let pool = AsyncClients::default();
    for value in ["one", "two"] {
        let resource = resource(&origin, value);
        let client = pool.get(&resource).unwrap();
        let bytes = client
            .get(resource.url())
            .headers(resource.headers().clone())
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        assert_eq!(&bytes[..], b"a");
    }
    worker.join().unwrap();
}

#[test]
fn independent_runtimes_use_separate_probe_pools_on_the_same_origin() {
    let (origin, worker) = keep_alive_connections(2);
    for _ in 0..2 {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let pool = AsyncClients::default();
            let clone = pool.clone();
            assert!(Arc::ptr_eq(&pool.cache, &clone.cache));
            for (value, pool) in [("one", &pool), ("two", &clone)] {
                let resource = resource(&origin, value);
                let bytes = pool
                    .get(&resource)
                    .unwrap()
                    .get(resource.url())
                    .headers(resource.headers().clone())
                    .send()
                    .await
                    .unwrap()
                    .bytes()
                    .await
                    .unwrap();
                assert_eq!(&bytes[..], b"a");
            }
        });
    }
    worker.join().unwrap();
}

#[test]
fn concurrent_client_misses_build_without_locking_hits_and_insert_only_once() {
    use std::sync::mpsc;
    let cache = Arc::new(ClientCache::<usize>::default());
    let hot = HttpResource::new("https://hot.test/audio");
    assert_eq!(cached(&cache, &hot, |_| Ok(7)).unwrap(), 7);
    let (started_tx, started_rx) = mpsc::channel();
    let mut releases = Vec::new();
    let mut workers = Vec::new();
    for id in [1, 2] {
        let cache = cache.clone();
        let started = started_tx.clone();
        let (release_tx, release_rx) = mpsc::channel();
        releases.push(release_tx);
        workers.push(thread::spawn(move || {
            cached(
                &cache,
                &HttpResource::new("https://cold.test/audio"),
                |_| {
                    started.send(()).unwrap();
                    release_rx
                        .recv_timeout(Duration::from_secs(3))
                        .map_err(|_| anyhow!("build gate timed out"))?;
                    Ok(id)
                },
            )
            .unwrap()
        }));
    }
    for _ in 0..2 {
        started_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("both builds should start concurrently");
    }
    assert_eq!(
        cached(&cache, &hot, |_| panic!("hot client must still hit")).unwrap(),
        7
    );
    for release in releases {
        release.send(()).unwrap();
    }
    let first = workers.remove(0).join().unwrap();
    assert_eq!(workers.remove(0).join().unwrap(), first);
    assert_eq!(cache.lock().unwrap().len(), 2);
}

#[test]
fn evicted_clients_are_dropped_after_releasing_the_pool_lock() {
    use std::sync::Weak;
    struct Client {
        cache: Weak<ClientCache<Arc<Client>>>,
    }
    impl Drop for Client {
        fn drop(&mut self) {
            if let Some(cache) = self.cache.upgrade() {
                assert!(cache.try_lock().is_ok(), "client dropped with pool locked");
            }
        }
    }
    let cache = Arc::new(ClientCache::default());
    for index in 0..=MAX_CLIENTS {
        cached(
            &cache,
            &HttpResource::new(format!("https://host{index}.test/audio")),
            |_| {
                Ok(Arc::new(Client {
                    cache: Arc::downgrade(&cache),
                }))
            },
        )
        .unwrap();
    }
    assert_eq!(cache.lock().unwrap().len(), MAX_CLIENTS);
}

/// A one-connection SOCKS5 proxy (no authentication) that answers the tunnelled
/// HTTP request itself and reports the CONNECT target it was asked for.
fn socks5_fixture() -> (String, thread::JoinHandle<String>) {
    use std::io::Read;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("socks5h://{}", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut greeting = [0; 2];
        stream.read_exact(&mut greeting).unwrap();
        assert_eq!(greeting[0], 5);
        let mut methods = vec![0; greeting[1] as usize];
        stream.read_exact(&mut methods).unwrap();
        assert!(methods.contains(&0), "client offers no-auth");
        stream.write_all(&[5, 0]).unwrap();
        let mut request = [0; 4];
        stream.read_exact(&mut request).unwrap();
        assert_eq!(&request[..3], &[5, 1, 0], "CONNECT");
        assert_eq!(request[3], 3, "socks5h sends the host name");
        let mut length = [0; 1];
        stream.read_exact(&mut length).unwrap();
        let mut host = vec![0; length[0] as usize];
        stream.read_exact(&mut host).unwrap();
        let mut port = [0; 2];
        stream.read_exact(&mut port).unwrap();
        stream.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        while reader.read_line(&mut line).unwrap() > 0 && line != "\r\n" {
            line.clear();
        }
        reader
            .get_mut()
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nsocks")
            .unwrap();
        format!(
            "{}:{}",
            String::from_utf8(host).unwrap(),
            u16::from_be_bytes(port)
        )
    });
    (address, worker)
}

#[test]
fn socks5_routes_tunnel_requests_by_host_name() {
    let (proxy, worker) = socks5_fixture();
    let resource =
        HttpResource::new("http://music.invalid/audio").with_route(NetworkRoute::Proxy(proxy));
    let client = blocking_cached(&resource, &ClientCache::default()).unwrap();
    let body = client.get(resource.url()).send().unwrap().text().unwrap();
    assert_eq!(body, "socks");
    assert_eq!(worker.join().unwrap(), "music.invalid:80");
}

#[test]
fn proxy_routes_never_embed_credentials() {
    for url in [
        "socks5://user:secret@127.0.0.1:1080",
        "http://user:secret@127.0.0.1:7890",
    ] {
        let route = NetworkRoute::Proxy(url.into());
        assert!(route_async(reqwest::Client::builder(), &route).is_err());
        assert!(route_blocking(reqwest::blocking::Client::builder(), &route).is_err());
    }
}
