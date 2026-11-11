use super::*;
use crate::source::{ProviderKind, SourceId};

fn media(source: &str) -> MediaKey {
    MediaKey::new(
        SourceId::new(ProviderKind::OpenSubsonic, source),
        "same-song",
    )
}
fn record(id: u64) -> Record {
    Record {
        id,
        track_name: "歌曲".into(),
        artist_name: "歌手".into(),
        duration: 223.0,
        synced_lyrics: Some("[00:01]test".into()),
        ..Default::default()
    }
}
fn query() -> Query {
    Query {
        title: "歌曲".into(),
        artist: "歌手".into(),
        duration: Some(223),
        ..Default::default()
    }
}

#[test]
fn clear_and_restart_preserve_pins_and_source_scoped_offsets() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.json");
    let cache = LyricsCache::at(path.clone());
    let mut document = Document::online(record(1));
    document.manual = true;
    document.offset_ms = -600;
    document.plain = true;
    document.query = Some(query());
    cache.remember(&media("a"), &document).unwrap();
    cache.save_automatic(&query(), Some(record(2))).unwrap();
    assert_eq!(cache.stats().0, 2);
    cache.clear_automatic().unwrap();
    cache.flush().unwrap();
    assert_eq!(cache.stats().0, 1);
    let cache = LyricsCache::at(path);
    let pin = cache.selected(&media("a")).unwrap();
    assert_eq!(pin.offset_ms, -600);
    assert!(pin.plain);
    assert_eq!(pin.record.unwrap().id, 1);
    assert!(cache.selected(&media("b")).is_none());
    assert!(cache.automatic(&query()).is_none());
    cache.restore(&media("a")).unwrap();
    cache.flush().unwrap();
    assert_eq!(cache.stats().0, 0);
    assert_eq!(cache.preference(&media("a")).offset_ms, 0);
}

#[test]
fn metadata_and_algorithm_queries_expire_without_expiring_pins() {
    let directory = tempfile::tempdir().unwrap();
    let cache = LyricsCache::at(directory.path().join("cache.json"));
    cache.save_automatic(&query(), None).unwrap();
    assert_eq!(cache.automatic(&query()), Some(None));
    assert!(cache
        .automatic(&Query {
            duration: Some(230),
            ..query()
        })
        .is_none());
    cache
        .change(|s| s.queries.get_mut(&query_key(&query())).unwrap().saved = now() - MISS_TTL)
        .unwrap();
    assert!(cache.automatic(&query()).is_none());
    cache.save_automatic(&query(), Some(record(1))).unwrap();
    cache
        .change(|s| s.queries.get_mut(&query_key(&query())).unwrap().saved = now() - SUCCESS_TTL)
        .unwrap();
    assert!(cache.automatic(&query()).is_none());
}

#[test]
fn failed_save_retains_pending_preferences_reports_failure_and_can_retry() {
    let directory = tempfile::tempdir().unwrap();
    let blocked = directory.path().join("file");
    std::fs::write(&blocked, "blocking parent").unwrap();
    let cache = LyricsCache::at(blocked.join("cache.json"));
    cache
        .remember(&media("a"), &Document::online(record(1)))
        .unwrap();
    assert!(cache.flush().is_err());
    assert!(cache.take_error().is_some());
    assert!(cache.take_error().is_none());
    assert!(cache.selected(&media("a")).is_some());
    // A failed background save does not silently discard a user's choice.
    std::fs::remove_file(&blocked).unwrap();
    cache.flush().unwrap();
    assert_eq!(
        LyricsCache::at(blocked.join("cache.json"))
            .selected(&media("a"))
            .unwrap()
            .record
            .unwrap()
            .id,
        1
    );
}

#[test]
fn corrupt_and_future_version_caches_can_be_rebuilt() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.json");
    for value in [
        "{broken",
        "{\"version\":999,\"bodies\":{},\"queries\":{},\"songs\":{}}",
    ] {
        std::fs::write(&path, value).unwrap();
        let cache = LyricsCache::at(path.clone());
        assert_eq!(cache.stats(), (0, 0));
        cache.save_automatic(&query(), Some(record(1))).unwrap();
        cache.flush().unwrap();
        assert!(LyricsCache::at(path.clone())
            .automatic(&query())
            .unwrap()
            .is_some());
    }
}

#[test]
fn automatic_display_preferences_survive_clearing_without_pinning_the_body() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.json");
    let cache = LyricsCache::at(path.clone());
    cache.save_automatic(&query(), Some(record(1))).unwrap();
    let mut document = Document::native(record(1).lyrics());
    document.plain = true;
    document.offset_ms = 750;
    cache.remember(&media("a"), &document).unwrap();
    assert!(cache.selected(&media("a")).is_none());
    cache.clear_automatic().unwrap();
    cache.flush().unwrap();
    let restored = LyricsCache::at(path);
    assert_eq!(restored.stats(), (0, 0));
    assert_eq!(restored.preference(&media("a")).offset_ms, 750);
    assert!(restored.preference(&media("a")).plain);
    assert_eq!(restored.preference(&media("b")).offset_ms, 0);
}

#[test]
fn unreadable_selected_bodies_and_untrusted_automatic_records_are_cache_misses() {
    let directory = tempfile::tempdir().unwrap();
    let cache = LyricsCache::at(directory.path().join("cache.json"));
    cache
        .remember(
            &media("a"),
            &Document::online(Record {
                id: 1,
                ..Default::default()
            }),
        )
        .unwrap();
    assert!(cache.selected(&media("a")).is_none());
    cache
        .save_automatic(
            &query(),
            Some(Record {
                artist_name: "Other artist".into(),
                ..record(1)
            }),
        )
        .unwrap();
    assert!(cache.automatic(&query()).is_none());
}

#[test]
fn blocked_background_save_does_not_block_readers_and_coalesces_newer_changes() {
    use std::sync::{mpsc, Mutex};
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let saved = Arc::new(Mutex::new(Vec::new()));
    let snapshots = saved.clone();
    let caller = std::thread::current().id();
    let mut first = true;
    let writer = Writer::start(
        Store {
            version: VERSION,
            ..Default::default()
        },
        move |store| {
            assert_ne!(std::thread::current().id(), caller);
            if first {
                first = false;
                entered_tx.send(()).unwrap();
                release_rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
            }
            snapshots.lock().unwrap().push(store.clone());
            Ok(())
        },
    )
    .unwrap();
    let cache = LyricsCache {
        writer: Arc::new(writer),
    };
    let mut document = Document::online(record(1));
    cache.remember(&media("a"), &document).unwrap();
    entered_rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    for offset in 1..=100 {
        document.offset_ms = offset;
        cache.remember(&media("a"), &document).unwrap();
    }
    // These calls finish while the saver is still blocked. The writer holds
    // no UI read lock during I/O, and newer choices are immediately visible.
    cache.save_automatic(&query(), Some(record(2))).unwrap();
    cache.clear_automatic().unwrap();
    document.plain = true;
    cache.remember(&media("a"), &document).unwrap();
    assert_eq!(cache.preference(&media("a")).offset_ms, 100);
    assert!(cache.selected(&media("a")).unwrap().plain);
    assert!(cache.automatic(&query()).is_none());
    release_tx.send(()).unwrap();
    cache.flush().unwrap();
    let saved = saved.lock().unwrap();
    assert_eq!(
        saved.len(),
        2,
        "all edits during the blocked save coalesce into one snapshot"
    );
    let latest = saved.last().unwrap();
    assert_eq!(latest.songs[&digest(&media("a"))].offset_ms, 100);
    assert!(latest.songs[&digest(&media("a"))].plain);
    assert_eq!(latest.bodies.len(), 1);
    assert!(latest.queries.is_empty());
    assert_eq!(cache.stats().0, 1);
}

#[test]
fn shutdown_flushes_pending_changes_and_the_latest_restore() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("cache.json");
    let cache = LyricsCache::at(path.clone());
    let clone = cache.clone();
    cache
        .remember(&media("a"), &Document::online(record(1)))
        .unwrap();
    cache.save_automatic(&query(), Some(record(2))).unwrap();
    clone.restore(&media("a")).unwrap();
    drop(clone);
    drop(cache);
    let restored = LyricsCache::at(path);
    assert!(restored.selected(&media("a")).is_none());
    assert_eq!(restored.stats().0, 1);
    assert_eq!(restored.automatic(&query()).unwrap().unwrap().id, 2);
}

#[test]
fn snapshots_share_large_immutable_bodies_and_stats_use_recorded_sizes() {
    let mut store = Store {
        version: VERSION,
        ..Default::default()
    };
    let record = Record {
        plain_lyrics: Some("a".repeat(MAX_BYTES)),
        ..record(1)
    };
    let bytes = serde_json::to_vec(&record).unwrap().len();
    store.insert_body("large".into(), record, bytes);
    store.songs.insert(
        digest(&media("a")),
        Preference {
            body: Some("large".into()),
            ..Default::default()
        },
    );
    let original = store.bodies["large"].clone();
    let writer = Writer::start(store, move |snapshot| {
        assert!(Arc::ptr_eq(&original, &snapshot.bodies["large"]));
        Ok(())
    })
    .unwrap();
    let cache = LyricsCache {
        writer: Arc::new(writer),
    };
    cache
        .remember(
            &media("a"),
            &Document::native(crate::models::Lyrics::default()),
        )
        .unwrap();
    assert_eq!(cache.stats(), (1, bytes));
    cache.flush().unwrap();
    assert_eq!(cache.stats(), (1, bytes));
}

#[test]
fn pruning_counts_shared_query_references_and_exempts_pinned_bodies() {
    let mut store = Store {
        version: VERSION,
        ..Default::default()
    };
    store.insert_body("pinned".into(), record(1), MAX_BYTES + 1);
    store.insert_body("shared".into(), record(2), MAX_BYTES);
    store.insert_body("recent".into(), record(3), 16);
    store.songs.insert(
        "song".into(),
        Preference {
            body: Some("pinned".into()),
            ..Default::default()
        },
    );
    for (key, body, saved) in [
        ("old", "shared", 1),
        ("other", "shared", 2),
        ("new", "recent", 3),
    ] {
        store.queries.insert(
            key.into(),
            Automatic {
                body: Some(body.into()),
                saved,
            },
        );
    }
    prune(&mut store);
    assert!(store.bodies.contains_key("pinned"));
    assert!(store.bodies.contains_key("recent"));
    assert!(!store.bodies.contains_key("shared"));
    assert_eq!(store.queries.len(), 1);
    assert!(store.queries.contains_key("new"));
    assert_eq!(store.bytes, MAX_BYTES + 17);
    // Negative entries are bounded even though they have no body to reclaim.
    for index in 0..10_001 {
        store.queries.insert(
            index.to_string(),
            Automatic {
                body: None,
                saved: 4 + index,
            },
        );
    }
    prune(&mut store);
    assert_eq!(store.queries.len(), 10_000);
}
