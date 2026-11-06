use super::*;
use crate::audio::preload::tests::{client, wait_for, TestCache};
use crate::audio::stream_cache::cache_paths;
use std::fs;
use std::io::Write;
use std::net::TcpListener;

const AUDIO: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\naudio";
const RANGE_AUDIO: &[u8] = b"HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 0-4/5\r\nConnection: close\r\n\r\naudio";
const RANGE_TAIL: &[u8] = b"HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 1-4/5\r\nConnection: close\r\n\r\nudio";

pub(in crate::audio) fn serve<B: AsRef<[u8]> + Send + 'static>(
    responses: Vec<(u64, B)>,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/audio", listener.local_addr().unwrap());
    let server = thread::spawn(move || {
        for (start, response) in responses {
            let mut connection = None;
            wait_for(|| match listener.accept() {
                Ok((stream, _)) => {
                    connection = Some(stream);
                    true
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => false,
                Err(error) => panic!("HTTP fixture accept failed: {error}"),
            });
            let mut stream = connection.unwrap();
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
            let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
            assert!(
                request
                    .lines()
                    .any(|line| line == format!("range: bytes={start}-")),
                "{request}"
            );
            stream.write_all(response.as_ref()).unwrap();
        }
    });
    (url, server)
}

#[test]
fn completed_cache_is_read_without_network_access() {
    let cache = TestCache::new();
    let (complete_path, _) = cache_paths(&cache.0, "server:song-1");
    fs::write(&complete_path, b"cached audio bytes").unwrap();
    let mut reader = HttpRangeReader::open(
        client(),
        "http://127.0.0.1:1/unreachable".into(),
        &cache.0,
        "server:song-1",
    )
    .unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"cached audio bytes");
    assert!(reader.is_seekable());
}

#[test]
fn completed_cache_read_failure_reaches_the_decoder_diagnostic_ledger() {
    let cache = TestCache::new();
    let (complete, _) = cache_paths(&cache.0, "shortened-complete");
    fs::write(&complete, b"audio").unwrap();
    let control = StreamControl::new(false);
    let mut reader = HttpRangeReader::open_controlled(
        client(),
        "http://127.0.0.1:1/unused".into(),
        &cache.0,
        "shortened-complete",
        control.clone(),
    )
    .unwrap();
    fs::write(complete, b"").unwrap();
    let error = reader.read(&mut [0; 5]).unwrap_err();
    assert_eq!(classify_io(&error), StreamFailure::Truncated);
    assert_eq!(
        control.take_io_error().unwrap().kind,
        StreamFailure::Truncated
    );
}

#[test]
fn shortened_partial_cache_falls_back_to_the_open_network_response() {
    let cache = TestCache::new();
    let (complete, partial) = cache_paths(&cache.0, "shortened-partial");
    fs::write(&partial, b"au").unwrap();
    let (url, server) = serve(vec![(0, RANGE_AUDIO)]);
    let mut reader = HttpRangeReader::open(client(), url, &cache.0, "shortened-partial").unwrap();
    let progress = reader.buffer_progress();
    assert_eq!(progress.cached_bytes(), 2);
    fs::write(&partial, b"").unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    server.join().unwrap();
    assert_eq!(bytes, b"audio");
    assert_eq!(progress.cached_bytes(), 0);
    drop(reader);
    assert!(!complete.exists());
}

#[test]
fn shortening_partial_cache_after_reading_resumes_at_the_same_byte_position() {
    let cache = TestCache::new();
    let (_, partial) = cache_paths(&cache.0, "shortened-partial");
    fs::write(&partial, b"au").unwrap();
    let (url, server) = serve(vec![(0, RANGE_AUDIO), (1, RANGE_TAIL)]);
    let mut reader = HttpRangeReader::open(client(), url, &cache.0, "shortened-partial").unwrap();
    let mut first = [0];
    reader.read_exact(&mut first).unwrap();
    assert_eq!(first, *b"a");
    fs::write(&partial, b"").unwrap();
    let mut tail = Vec::new();
    reader.read_to_end(&mut tail).unwrap();
    server.join().unwrap();
    assert_eq!(tail, b"udio");
    assert_eq!(reader.stream_position().unwrap(), 5);
}

#[test]
fn cache_write_failure_falls_back_to_network_playback() {
    let cache = TestCache::new();
    let invalid_cache_dir = cache.0.join("not-a-directory");
    fs::write(&invalid_cache_dir, b"not a directory").unwrap();
    let (url, server) = serve(vec![(0, AUDIO)]);
    let mut reader =
        HttpRangeReader::open(client(), url, &invalid_cache_dir, "server:song-1").unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    server.join().unwrap();
    assert_eq!(bytes, b"audio");
    assert_eq!(reader.buffer_progress().cached_bytes(), 0);
    assert_eq!(fs::read(invalid_cache_dir).unwrap(), b"not a directory");
}

#[test]
fn wrong_resume_range_never_appends_or_finalizes_the_response_body() {
    let cache = TestCache::new();
    let (complete, partial) = cache_paths(&cache.0, "wrong-range");
    fs::write(&partial, b"au").unwrap();
    let wrong = b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 0-2/5\r\nConnection: close\r\n\r\naud";
    let (url, server) = serve(vec![(0, RANGE_AUDIO), (2, wrong)]);
    let control = StreamControl::new(false);
    let mut reader =
        HttpRangeReader::open_controlled(client(), url, &cache.0, "wrong-range", control.clone())
            .unwrap();
    let mut bytes = Vec::new();
    let result = reader.read_to_end(&mut bytes);
    server.join().unwrap();
    assert!(result.is_err(), "incorrectly accepted bytes: {bytes:?}");
    assert_eq!(classify_io(&result.unwrap_err()), StreamFailure::Truncated);
    assert_eq!(
        control.take_io_error().unwrap().kind,
        StreamFailure::Truncated
    );
    drop(reader);
    assert_eq!(fs::read(partial).unwrap(), b"au");
    assert!(!complete.exists());
}

#[test]
fn wrong_initial_range_is_rejected_before_opening_a_partial_cache() {
    let cache = TestCache::new();
    let (_, partial) = cache_paths(&cache.0, "wrong-range");
    let wrong = b"HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 1-4/5\r\nConnection: close\r\n\r\nudio";
    let (url, server) = serve(vec![(0, wrong)]);
    let result = HttpRangeReader::open(client(), url, &cache.0, "wrong-range");
    server.join().unwrap();
    assert!(result.is_err());
    assert!(!partial.exists());
}

#[test]
fn a_range_with_unknown_total_does_not_use_payload_length_as_resource_length() {
    let cache = TestCache::new();
    let unknown = b"HTTP/1.1 206 Partial Content\r\nContent-Length: 3\r\nContent-Range: bytes 0-2/*\r\nConnection: close\r\n\r\naud";
    let (url, server) = serve(vec![(0, unknown)]);
    let reader = HttpRangeReader::open(client(), url, &cache.0, "unknown-total").unwrap();
    server.join().unwrap();
    assert_eq!(reader.len(), None);
    assert!(!reader.is_seekable());
}

#[test]
fn changed_known_length_is_rejected_before_resumed_bytes_are_cached() {
    let cache = TestCache::new();
    let (complete, partial) = cache_paths(&cache.0, "changed-length");
    fs::write(&partial, b"au").unwrap();
    let changed = b"HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 2-5/6\r\nConnection: close\r\n\r\ndioX";
    let (url, server) = serve(vec![(0, RANGE_AUDIO), (2, changed)]);
    let mut reader = HttpRangeReader::open(client(), url, &cache.0, "changed-length").unwrap();
    let mut bytes = Vec::new();
    let error = reader.read_to_end(&mut bytes).unwrap_err();
    server.join().unwrap();
    assert_eq!(classify_io(&error), StreamFailure::Truncated);
    assert!(error.to_string().contains("length changed from 5 to 6"));
    drop(reader);
    assert!(!complete.exists());
    assert_eq!(fs::read(partial).unwrap(), b"au");
}

#[test]
fn transcode_length_reestimation_preserves_the_existing_near_end_tolerance() {
    let cache = TestCache::new();
    let (complete, partial) = cache_paths(&cache.0, "transcoded");
    fs::write(&partial, [b'a'; 95]).unwrap();
    let initial = format!("HTTP/1.1 206 Partial Content\r\nContent-Length: 100\r\nContent-Range: bytes 0-99/100\r\nConnection: close\r\n\r\n{}", "a".repeat(100)).into_bytes();
    let resumed = b"HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 95-98/99\r\nConnection: close\r\n\r\ntail".to_vec();
    let (url, server) = serve(vec![(0, initial), (95, resumed)]);
    let mut reader = HttpRangeReader::open(
        client(),
        format!("{url}?maxBitRate=320"),
        &cache.0,
        "transcoded",
    )
    .unwrap();
    assert!(!reader.is_seekable());
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    server.join().unwrap();
    let expected = [vec![b'a'; 95], b"tail".to_vec()].concat();
    assert_eq!(bytes, expected);
    assert_eq!(reader.len(), Some(99));
    assert_eq!(reader.buffer_progress().total_bytes(), 99);
    drop(reader);
    assert_eq!(fs::read(complete).unwrap(), expected);
    assert!(!partial.exists());
}

#[test]
fn chunked_body_cannot_deliver_bytes_beyond_its_declared_range() {
    let cache = TestCache::new();
    let (complete, _) = cache_paths(&cache.0, "chunked-ranges");
    let first = b"HTTP/1.1 206 Partial Content\r\nTransfer-Encoding: chunked\r\nContent-Range: bytes 0-2/5\r\nConnection: close\r\n\r\n6\r\naudBAD\r\n0\r\n\r\n";
    let last = b"HTTP/1.1 206 Partial Content\r\nContent-Length: 2\r\nContent-Range: bytes 3-4/5\r\nConnection: close\r\n\r\nio";
    let (url, server) = serve(vec![(0, first.as_slice()), (3, last.as_slice())]);
    let mut reader = HttpRangeReader::open(client(), url, &cache.0, "chunked-ranges").unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();
    server.join().unwrap();
    assert_eq!(bytes, b"audio");
    drop(reader);
    assert_eq!(fs::read(complete).unwrap(), b"audio");
}
