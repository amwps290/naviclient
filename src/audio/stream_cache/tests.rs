use super::*;
use crate::audio::preload::tests::TestCache;

fn partial(cache: &TestCache, control: Arc<StreamControl>, len: Option<u64>) -> StreamCache {
    let mut stream = StreamCache::open(&cache.0, "song", control).unwrap();
    stream.prepare_partial(len, true);
    stream
}

fn read_all(stream: &mut StreamCache) -> Vec<u8> {
    let mut bytes = vec![0; stream.cached_len() as usize];
    let len = stream.read(0, &mut bytes).unwrap().unwrap();
    bytes.truncate(len);
    bytes
}

#[test]
fn foreground_partial_survives_drop_then_resumes_and_publishes_exact_bytes() {
    let cache = TestCache::new();
    let (complete, part) = cache_paths(&cache.0, "song");
    let mut first = partial(&cache, StreamControl::new(false), Some(5));
    first.append(0, b"au");
    drop(first);
    assert!(!complete.exists());
    assert_eq!(fs::read(&part).unwrap(), b"au");

    let mut resumed = partial(&cache, StreamControl::new(false), Some(5));
    assert_eq!(resumed.cached_len(), 2);
    assert_eq!(read_all(&mut resumed), b"au");
    resumed.append(2, b"dio");
    drop(resumed);
    assert!(!part.exists());
    assert_eq!(fs::read(complete).unwrap(), b"audio");
}

#[test]
fn gaps_and_duplicate_network_bytes_cannot_extend_the_cached_prefix() {
    let cache = TestCache::new();
    let (complete, part) = cache_paths(&cache.0, "song");
    let mut stream = partial(&cache, StreamControl::new(false), Some(5));
    stream.append(0, b"au");
    stream.append(4, b"o");
    stream.append(0, b"incorrect");
    stream.append(2, b"");
    assert_eq!(stream.cached_len(), 2);
    assert_eq!(stream.progress().cached_bytes(), 2);
    assert_eq!(fs::read(&part).unwrap(), b"au");
    stream.append(2, b"dio");
    drop(stream);
    assert_eq!(fs::read(complete).unwrap(), b"audio");
}

#[test]
fn unsupported_ranges_or_oversized_partials_reset_the_file_before_writing() {
    for (len, range_supported) in [(Some(12), false), (Some(5), true), (None, false)] {
        let cache = TestCache::new();
        let (complete, part) = cache_paths(&cache.0, "song");
        fs::write(&part, b"stale audio").unwrap();
        let mut stream = StreamCache::open(&cache.0, "song", StreamControl::new(false)).unwrap();
        stream.prepare_partial(len, range_supported);
        assert_eq!(stream.cached_len(), 0);
        assert_eq!(fs::read(&part).unwrap(), b"");
        stream.append(0, b"audio");
        drop(stream);
        let actual = if len == Some(5) { complete } else { part };
        assert_eq!(fs::read(actual).unwrap(), b"audio");
    }
}

#[test]
fn completed_cache_stays_read_only_and_does_not_touch_an_existing_partial() {
    let cache = TestCache::new();
    let (complete, part) = cache_paths(&cache.0, "song");
    fs::write(&complete, b"audio").unwrap();
    fs::write(&part, b"another active reader").unwrap();
    let mut stream = StreamCache::open(&cache.0, "song", StreamControl::new(true)).unwrap();
    assert!(stream.complete());
    assert_eq!(stream.cached_len(), 5);
    assert_eq!(read_all(&mut stream), b"audio");
    stream.append(5, b"incorrect");
    drop(stream);
    assert_eq!(fs::read(complete).unwrap(), b"audio");
    assert_eq!(fs::read(part).unwrap(), b"another active reader");
}

#[test]
fn speculative_files_are_unique_and_cancellation_preserves_active_partial() {
    let cache = TestCache::new();
    let (complete, active_part) = cache_paths(&cache.0, "song");
    fs::write(&active_part, b"active reader").unwrap();
    let first_control = StreamControl::new(true);
    let mut first = partial(&cache, first_control.clone(), Some(5));
    let mut second = partial(&cache, StreamControl::new(true), Some(5));
    assert_ne!(first.partial_path, second.partial_path);
    assert_ne!(first.partial_path, active_part);
    assert_ne!(second.partial_path, active_part);
    let first_part = first.partial_path.clone();
    let second_part = second.partial_path.clone();
    first.append(0, b"audio");
    second.append(0, b"a");
    drop(second);
    assert!(!second_part.exists());
    assert_eq!(fs::read(&first_part).unwrap(), b"audio");
    assert_eq!(fs::read(&active_part).unwrap(), b"active reader");
    first_control.mark_fully_decoded();
    drop(first);
    assert!(!first_part.exists());
    assert_eq!(fs::read(complete).unwrap(), b"audio");
    assert_eq!(fs::read(active_part).unwrap(), b"active reader");
}

#[test]
fn a_promoted_but_unverified_preload_cannot_publish_even_all_downloaded_bytes() {
    let cache = TestCache::new();
    let (complete, _) = cache_paths(&cache.0, "song");
    let control = StreamControl::new(true);
    let mut stream = partial(&cache, control.clone(), Some(5));
    let part = stream.partial_path.clone();
    stream.append(0, b"audio");
    control.promote();
    control.cancel();
    drop(stream);
    assert!(!complete.exists());
    assert!(!part.exists());
}

#[test]
fn decode_completion_does_not_publish_a_truncated_preload() {
    let cache = TestCache::new();
    let (complete, _) = cache_paths(&cache.0, "song");
    let control = StreamControl::new(true);
    let mut stream = partial(&cache, control.clone(), Some(10));
    let part = stream.partial_path.clone();
    stream.append(0, b"audio");
    control.mark_fully_decoded();
    drop(stream);
    assert!(!complete.exists());
    assert!(!part.exists());
}

#[test]
fn actual_transcode_end_updates_shared_progress_and_finalization_length() {
    let cache = TestCache::new();
    let (complete, part) = cache_paths(&cache.0, "song");
    let mut stream = partial(&cache, StreamControl::new(false), Some(100));
    let progress = stream.progress();
    stream.append(0, &[1; 95]);
    assert_eq!(
        progress.buffered_duration(Duration::from_secs(100)),
        Duration::from_secs(95)
    );
    stream.finish_estimated_stream(95);
    assert_eq!(progress.total_bytes(), 95);
    assert_eq!(
        progress.buffered_duration(Duration::from_secs(100)),
        Duration::from_secs(100)
    );
    drop(stream);
    assert!(!part.exists());
    assert_eq!(fs::read(complete).unwrap(), [1; 95]);
}

#[test]
fn unknown_or_zero_length_does_not_publish_without_a_verified_end() {
    for len in [None, Some(0)] {
        let cache = TestCache::new();
        let (complete, part) = cache_paths(&cache.0, "song");
        let mut stream = partial(&cache, StreamControl::new(false), len);
        stream.append(0, b"audio");
        assert_eq!(
            stream.progress().buffered_duration(Duration::from_secs(12)),
            Duration::ZERO
        );
        drop(stream);
        assert!(!complete.exists());
        assert_eq!(fs::read(part).unwrap(), b"audio");
    }
}

#[test]
fn write_failure_disables_cache_progress_and_preserves_network_fallback() {
    let cache = TestCache::new();
    let (complete, part) = cache_paths(&cache.0, "song");
    fs::write(&part, b"au").unwrap();
    let mut stream = partial(&cache, StreamControl::new(false), Some(5));
    let progress = stream.progress();
    // A read-only handle makes writes fail without platform-specific devices.
    stream.file = Some(File::open(&part).unwrap());
    stream.append(2, b"dio");
    assert_eq!(stream.cached_len(), 0);
    assert_eq!(progress.cached_bytes(), 0);
    assert_eq!(stream.read(0, &mut [0; 5]).unwrap(), None);
    drop(stream);
    assert!(!complete.exists());
    assert_eq!(fs::read(part).unwrap(), b"au");
}

#[test]
fn reset_failure_disables_cache_and_preserves_the_original_partial_file() {
    let cache = TestCache::new();
    let (complete, part) = cache_paths(&cache.0, "song");
    fs::write(&part, b"stale cache").unwrap();
    let mut stream = partial(&cache, StreamControl::new(false), Some(20));
    let progress = stream.progress();
    stream.file = Some(File::open(&part).unwrap());
    stream.discard_partial();
    stream.append(0, b"audio");
    assert_eq!(stream.read(0, &mut [0; 5]).unwrap(), None);
    assert_eq!(progress.cached_bytes(), 0);
    drop(stream);
    assert!(!complete.exists());
    assert_eq!(fs::read(part).unwrap(), b"stale cache");
}

#[test]
fn truncated_completed_cache_returns_an_error_instead_of_a_false_end() {
    let cache = TestCache::new();
    let (complete, _) = cache_paths(&cache.0, "song");
    fs::write(&complete, b"audio").unwrap();
    let mut stream = StreamCache::open(&cache.0, "song", StreamControl::new(false)).unwrap();
    fs::write(&complete, b"").unwrap();
    let error = stream.read(0, &mut [0; 5]).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    assert!(stream.complete());
    // An empty caller buffer is still an ordinary empty read.
    assert_eq!(stream.read(0, &mut []).unwrap(), Some(0));
}

#[test]
fn pruning_removes_oldest_files_to_the_limit_and_preserves_directories() {
    let cache = TestCache::new();
    let oldest = cache.0.join("old.part");
    let newest = cache.0.join("new.audio");
    fs::write(&oldest, b"older").unwrap();
    fs::write(&newest, b"newer").unwrap();
    OpenOptions::new()
        .write(true)
        .open(&oldest)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1))
        .unwrap();
    OpenOptions::new()
        .write(true)
        .open(&newest)
        .unwrap()
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(2))
        .unwrap();
    let directory = cache.0.join("covers");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("cover"), b"preserve nested files").unwrap();
    prune_to(&cache.0, 5).unwrap();
    assert!(!oldest.exists());
    assert_eq!(fs::read(newest).unwrap(), b"newer");
    assert_eq!(
        fs::read(directory.join("cover")).unwrap(),
        b"preserve nested files"
    );
}

#[test]
fn preparing_an_absent_directory_creates_it_and_retains_files_below_limit() {
    let cache = TestCache::new();
    let nested = cache.0.join("audio");
    prepare_cache_dir(&nested).unwrap();
    let path = nested.join("song.part");
    fs::write(&path, b"audio").unwrap();
    prune_to(&nested, 5).unwrap();
    assert_eq!(fs::read(path).unwrap(), b"audio");
}

#[test]
fn byte_queries_keep_raw_counts_while_buffered_duration_clamps_to_known_length() {
    for (cached, total, expected_seconds) in [
        (0, None, 0),
        (u64::MAX, None, 0),
        (u64::MAX, Some(0), 0),
        (25, Some(100), 50),
        (150, Some(100), 200),
        (u64::MAX, Some(u64::MAX), 200),
    ] {
        let progress = BufferProgress::new(cached, total);
        assert_eq!(progress.cached_bytes(), cached);
        assert_eq!(progress.total_bytes(), total.unwrap_or(0));
        assert_eq!(
            progress.buffered_duration(Duration::from_secs(200)),
            Duration::from_secs(expected_seconds)
        );
    }
}

#[test]
fn shared_queries_follow_cache_owned_prefix_growth_and_estimated_end() {
    let cache = TestCache::new();
    let mut stream = partial(&cache, StreamControl::new(false), Some(100));
    let progress = stream.progress();
    let observer = progress.clone();
    assert_eq!(observer.cached_bytes(), 0);
    assert_eq!(observer.total_bytes(), 100);
    stream.append(0, b"au");
    assert_eq!(observer.cached_bytes(), 2);
    stream.append(4, b"o");
    assert_eq!(observer.cached_bytes(), 2);
    stream.append(2, b"dio");
    assert_eq!(observer.cached_bytes(), 5);
    stream.finish_estimated_stream(5);
    assert_eq!(observer.total_bytes(), 5);
    assert_eq!(
        observer.buffered_duration(Duration::from_secs(12)),
        Duration::from_secs(12)
    );
    drop(stream);
    assert_eq!(fs::read(cache_paths(&cache.0, "song").0).unwrap(), b"audio");
    assert_eq!(observer.cached_bytes(), 5);
    assert_eq!(observer.total_bytes(), 5);
}

#[test]
fn buffer_progress_maps_cached_bytes_to_duration() {
    let progress = BufferProgress::new(25, Some(100));
    assert_eq!(
        progress.buffered_duration(Duration::from_secs(200)),
        Duration::from_secs(50)
    );
}

#[test]
fn cache_paths_are_stable_and_server_specific() {
    let cache_dir = Path::new("cache");
    let first = cache_paths(cache_dir, "https://one.example:song-1");
    let same = cache_paths(cache_dir, "https://one.example:song-1");
    let other_server = cache_paths(cache_dir, "https://two.example:song-1");

    assert_eq!(first, same);
    assert_ne!(first, other_server);
    assert_eq!(
        first.0.extension().and_then(|value| value.to_str()),
        Some("audio")
    );
    assert_eq!(
        first.1.extension().and_then(|value| value.to_str()),
        Some("part")
    );
}
