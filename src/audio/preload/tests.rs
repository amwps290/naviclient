use super::*;
use std::net::{TcpListener, TcpStream};

struct TestCache(PathBuf);

impl TestCache {
    fn new() -> Self {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "navidrome-preload-test-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            SERIAL.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestCache {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn wait_for(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for audio worker"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn wav() -> Vec<u8> {
    // Twelve seconds of stereo 48 kHz PCM, larger than the HTTP read-ahead.
    let samples = 12 * 48_000 * 2;
    let size = samples * 2;
    let mut data = Vec::with_capacity(size + 44);
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&(size as u32 + 36).to_le_bytes());
    data.extend_from_slice(b"WAVEfmt ");
    data.extend_from_slice(&16_u32.to_le_bytes());
    data.extend_from_slice(&1_u16.to_le_bytes());
    data.extend_from_slice(&2_u16.to_le_bytes());
    data.extend_from_slice(&48_000_u32.to_le_bytes());
    data.extend_from_slice(&192_000_u32.to_le_bytes());
    data.extend_from_slice(&4_u16.to_le_bytes());
    data.extend_from_slice(&16_u16.to_le_bytes());
    data.extend_from_slice(b"data");
    data.extend_from_slice(&(size as u32).to_le_bytes());
    for _ in 0..samples {
        data.extend_from_slice(&8192_i16.to_le_bytes());
    }
    data
}

fn accept(listener: &TcpListener) -> TcpStream {
    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut request = Vec::new();
    while !request.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        request.push(byte[0]);
    }
    stream
}

fn send_audio(stream: &mut TcpStream) {
    send_bytes(stream, &wav());
}

fn send_bytes(stream: &mut TcpStream, data: &[u8]) {
    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", data.len()).unwrap();
    // Cancellation can close the socket while the server is still sending.
    let _ = stream.write_all(data);
}

fn client() -> Client {
    Client::builder()
        .no_proxy()
        .timeout(Duration::from_millis(250))
        .build()
        .unwrap()
}

fn wait_ready(preloader: &mut Preloader) {
    wait_for(|| {
        preloader.poll();
        preloader
            .entry
            .as_ref()
            .is_some_and(|entry| entry.ready.is_some())
    });
}

#[test]
fn ready_stream_sleeps_then_plays_all_samples_without_a_second_request() {
    let cache = TestCache::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!(
        "http://{}/stream?maxBitRate=128",
        listener.local_addr().unwrap()
    );
    let server = thread::spawn(move || {
        let mut stream = accept(&listener);
        // No second connection is possible, even on this non-range stream.
        drop(listener);
        send_audio(&mut stream);
    });
    let mut preloader = Preloader::new(client()).unwrap();
    preloader.request(
        7,
        url,
        "song:128".into(),
        Some(Duration::from_secs(12)),
        cache.0.clone(),
    );
    wait_ready(&mut preloader);
    let entry = preloader.entry.as_ref().unwrap();
    let stream = entry.ready.as_ref().unwrap();
    assert_eq!(stream.source.queued.len(), STARTUP_BUFFER_CHUNKS);
    assert!(matches!(
        stream.source.receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    assert!(!entry.control.fully_decoded.load(Ordering::Acquire));
    let bytes = stream.buffer_progress.cached_bytes.load(Ordering::Relaxed);
    assert!(bytes < wav().len() as u64);
    // Holding longer than the HTTP timeout must not expire an idle preparation.
    thread::sleep(Duration::from_millis(350));
    assert_eq!(
        stream.buffer_progress.cached_bytes.load(Ordering::Relaxed),
        bytes
    );
    let prepared = preloader.take(Some(7), "song:128", Duration::ZERO).unwrap();
    wait_for(|| {
        prepared
            .source
            .control
            .fully_decoded
            .load(Ordering::Acquire)
    });
    let error = prepared.error.clone();
    let samples: Vec<_> = prepared.source.collect();
    assert_eq!(samples.len(), 12 * 48_000 * 2);
    assert!(samples.iter().all(|sample| (*sample - 0.25).abs() < 0.001));
    assert!(error.lock().unwrap().is_none());
    server.join().unwrap();
    wait_for(|| cache_paths(&cache.0, "song:128").0.exists());
}

#[test]
fn cancelling_sleeping_decoder_preserves_current_songs_partial_cache() {
    let cache = TestCache::new();
    let (_, active_partial) = cache_paths(&cache.0, "same-song");
    fs::write(&active_partial, b"active reader owns these bytes").unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/stream", listener.local_addr().unwrap());
    let server = thread::spawn(move || send_audio(&mut accept(&listener)));
    let mut preloader = Preloader::new(client()).unwrap();
    preloader.request(1, url, "same-song".into(), None, cache.0.clone());
    wait_ready(&mut preloader);
    let control = preloader.entry.as_ref().unwrap().control.clone();
    preloader.cancel();
    assert!(preloader
        .take(Some(1), "same-song", Duration::ZERO)
        .is_none());
    wait_for(|| Arc::strong_count(&control) == 1);
    assert_eq!(
        fs::read(active_partial).unwrap(),
        b"active reader owns these bytes"
    );
    assert_eq!(fs::read_dir(&cache.0).unwrap().count(), 1);
    server.join().unwrap();
}

#[test]
fn rapid_replacements_run_only_latest_pending_request_and_discard_stale_result() {
    let cache = TestCache::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/stream", listener.local_addr().unwrap());
    let (accepted_tx, accepted_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let mut first = accept(&listener);
        accepted_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let _ = first.write_all(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\n\r\n");
        drop(first);
        let mut last = accept(&listener);
        drop(listener);
        send_audio(&mut last);
    });
    let mut preloader = Preloader::new(client()).unwrap();
    preloader.request(1, url.clone(), "same-song".into(), None, cache.0.clone());
    accepted_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    for id in 2..=100 {
        preloader.request(id, url.clone(), "same-song".into(), None, cache.0.clone());
    }
    release_tx.send(()).unwrap();
    wait_ready(&mut preloader);
    assert_eq!(preloader.entry.as_ref().unwrap().id, 100);
    // Identical song IDs in different queue occurrences cannot reuse old tickets.
    assert!(preloader
        .take(Some(1), "same-song", Duration::ZERO)
        .is_none());
    server.join().unwrap();
}

#[test]
fn failed_preload_allows_ordinary_playback_to_retry() {
    let cache = TestCache::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/stream", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        accept(&listener)
            .write_all(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        send_audio(&mut accept(&listener));
    });
    let mut preloader = Preloader::new(client()).unwrap();
    preloader.request(1, url.clone(), "song".into(), None, cache.0.clone());
    wait_for(|| {
        preloader.poll();
        preloader.entry.as_ref().unwrap().control.check().is_err()
    });
    assert!(preloader.take(Some(1), "song", Duration::ZERO).is_none());
    let prepared = play_url(
        client(),
        &url,
        &cache.0,
        "song",
        None,
        Duration::ZERO,
        StreamControl::new(false),
    )
    .unwrap();
    wait_for(|| {
        prepared
            .source
            .control
            .fully_decoded
            .load(Ordering::Acquire)
    });
    assert_eq!(prepared.source.count(), 12 * 48_000 * 2);
    server.join().unwrap();
}

#[test]
fn mismatched_quality_ticket_or_seek_never_promotes_prepared_audio() {
    let cache = TestCache::new();
    fs::write(cache_paths(&cache.0, "song:original").0, wav()).unwrap();
    let mut preloader = Preloader::new(client()).unwrap();
    for (ticket, key, position) in [
        (None, "song:original", Duration::ZERO),
        (Some(9), "song:original", Duration::ZERO),
        (Some(1), "song:128", Duration::ZERO),
        (Some(1), "song:original", Duration::from_secs(2)),
    ] {
        preloader.request(
            1,
            "http://127.0.0.1:1/unreachable".into(),
            "song:original".into(),
            None,
            cache.0.clone(),
        );
        wait_ready(&mut preloader);
        let control = preloader.entry.as_ref().unwrap().control.clone();
        assert!(preloader.take(ticket, key, position).is_none());
        wait_for(|| Arc::strong_count(&control) == 1);
        assert!(control.check().is_err());
    }
}

#[test]
fn truncated_preload_does_not_poison_cache_and_falls_back_to_fresh_audio() {
    let cache = TestCache::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/stream", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        // Valid WAV header, but only one of the advertised twelve seconds.
        send_bytes(&mut accept(&listener), &wav()[..44 + 48_000 * 4]);
        send_audio(&mut accept(&listener));
    });
    let mut preloader = Preloader::new(client()).unwrap();
    preloader.request(1, url.clone(), "song".into(), None, cache.0.clone());
    wait_for(|| {
        preloader.poll();
        preloader.entry.as_ref().unwrap().control.check().is_err()
    });
    let control = preloader.entry.as_ref().unwrap().control.clone();
    assert!(preloader.take(Some(1), "song", Duration::ZERO).is_none());
    wait_for(|| Arc::strong_count(&control) == 1);
    assert!(!cache_paths(&cache.0, "song").0.exists());
    let prepared = play_url(
        client(),
        &url,
        &cache.0,
        "song",
        None,
        Duration::ZERO,
        StreamControl::new(false),
    )
    .unwrap();
    wait_for(|| {
        prepared
            .source
            .control
            .fully_decoded
            .load(Ordering::Acquire)
    });
    assert_eq!(prepared.source.count(), 12 * 48_000 * 2);
    server.join().unwrap();
}

#[test]
fn cancellation_during_http_read_exits_within_io_timeout() {
    let cache = TestCache::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/stream", listener.local_addr().unwrap());
    let (accepted_tx, accepted_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let mut stream = accept(&listener);
        stream.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Type: audio/wav\r\nContent-Length: 1000000\r\nContent-Range: bytes 0-999999/1000000\r\n\r\nRIFF").unwrap();
        accepted_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    });
    let mut preloader = Preloader::new(client()).unwrap();
    preloader.request(1, url, "song".into(), None, cache.0.clone());
    accepted_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let control = preloader.entry.as_ref().unwrap().control.clone();
    preloader.cancel();
    wait_for(|| Arc::strong_count(&control) == 1);
    release_tx.send(()).unwrap();
    server.join().unwrap();
}

#[test]
fn cancelled_decoder_read_cannot_accumulate_speculative_workers() {
    let cache = TestCache::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/stream", listener.local_addr().unwrap());
    let (release_tx, release_rx) = mpsc::channel();
    let server = thread::spawn(move || {
        let mut first = accept(&listener);
        let data = wav();
        write!(
            first,
            "HTTP/1.1 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\n\r\n",
            data.len()
        )
        .unwrap();
        first.write_all(&data[..32 * 1024]).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(first);
        let mut last = accept(&listener);
        drop(listener);
        send_audio(&mut last);
    });
    let mut preloader = Preloader::new(client()).unwrap();
    preloader.request(1, url.clone(), "song".into(), None, cache.0.clone());
    let old = preloader.entry.as_ref().unwrap().control.clone();
    wait_for(|| old.state.lock().unwrap().running);
    for id in 2..=100 {
        preloader.request(id, url.clone(), "song".into(), None, cache.0.clone());
    }
    let latest = preloader.entry.as_ref().unwrap().control.clone();
    // Until the first read returns, the replacement decoder cannot start.
    assert!(!latest.state.lock().unwrap().running);
    release_tx.send(()).unwrap();
    wait_ready(&mut preloader);
    assert_eq!(preloader.entry.as_ref().unwrap().id, 100);
    assert!(!old.state.lock().unwrap().running);
    preloader.cancel();
    wait_for(|| Arc::strong_count(&latest) == 1);
    server.join().unwrap();
}
