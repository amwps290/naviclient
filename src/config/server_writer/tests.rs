use super::mailbox::CacheKey;
use super::*;
use crate::models::{QueueOrigin, Song};
use crate::recommendation::{RecommendationReason, RecommendedTrack};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant};

fn session_writer(
    mut save: impl FnMut(Option<&str>, &PlaybackSession) -> anyhow::Result<()> + Send + 'static,
) -> io::Result<ServerCacheWriter> {
    ServerCacheWriter::start_with(move |server, snapshot| match snapshot {
        CacheSnapshot::Session(session) => save(server, session),
        _ => panic!("expected a session snapshot"),
    })
}

fn session(position: f64) -> PlaybackSession {
    PlaybackSession {
        server_url: "http://fixture.test".into(),
        queue: vec![Song {
            id: "song".into(),
            duration: Some(180),
            ..Default::default()
        }],
        queue_origins: vec![QueueOrigin::Playlist],
        queue_index: Some(0),
        position_secs: position,
        ..Default::default()
    }
}

fn snapshots(server: &str, variant: u32) -> Vec<CacheSnapshot> {
    let song = Song {
        id: "song".into(),
        ..Default::default()
    };
    let mut history = ListeningHistory::default();
    history.record_started(song.clone(), 123);
    history.update_latest("song", variant, variant > 1);
    vec![
        CacheSnapshot::Session(session(variant as f64)),
        CacheSnapshot::Loudness(std::collections::HashMap::from([(
            "song".into(),
            (-16.0 + variant as f32, 0.5),
        )])),
        CacheSnapshot::Genres(vec![Genre {
            value: format!("Genre-{variant}"),
            ..Default::default()
        }]),
        CacheSnapshot::ListeningHistory(history),
        CacheSnapshot::DailyMix(DailyMixSnapshot {
            server_id: server.into(),
            day: "2026-11-04".into(),
            algorithm_version: 2,
            variant,
            generated_at: 123,
            tracks: vec![RecommendedTrack {
                song,
                reason: RecommendationReason::Favorite,
                score: 1.0,
                origin: QueueOrigin::DailyMix,
            }],
        }),
    ]
}

fn value(snapshot: &CacheSnapshot) -> serde_json::Value {
    match snapshot {
        CacheSnapshot::Session(data) => serde_json::to_value(data),
        CacheSnapshot::Loudness(data) => serde_json::to_value(data),
        CacheSnapshot::Genres(data) => serde_json::to_value(data),
        CacheSnapshot::ListeningHistory(data) => serde_json::to_value(data),
        CacheSnapshot::DailyMix(data) => serde_json::to_value(data),
    }
    .unwrap()
}

#[test]
fn mixed_caches_coalesce_by_server_and_kind_without_overwriting_other_families() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let saved = calls.clone();
    let mut first = true;
    let writer = ServerCacheWriter::start_with(move |server, snapshot| {
        if first {
            first = false;
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        saved
            .lock()
            .unwrap()
            .push((CacheKey::new(server, snapshot.kind()), value(snapshot)));
        Ok(())
    })
    .unwrap();
    writer.submit(Some("one"), session(0.0)).unwrap();
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    for variant in 1..=2 {
        for snapshot in snapshots("one", variant) {
            writer.submit(Some("ONE/"), snapshot).unwrap();
        }
    }
    let history = ListeningHistory::default();
    writer
        .submit(
            Some("two"),
            CacheSnapshot::ListeningHistory(history.clone()),
        )
        .unwrap();
    assert_eq!(writer.mailbox.pending_count(), 6);
    assert_eq!(
        writer.pending_session(Some("one")).unwrap().position_secs,
        2.0
    );
    assert_eq!(
        writer.pending_loudness(Some("one")).unwrap()["song"],
        (-14.0, 0.5)
    );
    assert_eq!(
        writer.pending_genres(Some("one")).unwrap()[0].value,
        "Genre-2"
    );
    let event = writer
        .pending_history(Some("one"))
        .unwrap()
        .events
        .remove(0);
    assert_eq!(event.listened_seconds, 2);
    assert!(event.completed);
    assert_eq!(writer.pending_daily_mix(Some("one")).unwrap().variant, 2);
    assert!(writer.pending_session(Some("two")).is_none());
    let mut expected = snapshots("one", 2)
        .into_iter()
        .map(|snapshot| {
            (
                CacheKey::new(Some("one"), snapshot.kind()),
                value(&snapshot),
            )
        })
        .collect::<BTreeMap<_, _>>();
    expected.insert(
        CacheKey::new(Some("two"), CacheKind::ListeningHistory),
        serde_json::to_value(history).unwrap(),
    );
    release_tx.send(()).unwrap();
    assert_eq!(writer.flush(), Ok(()));
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 7);
    assert_eq!(
        calls.iter().skip(1).cloned().collect::<BTreeMap<_, _>>(),
        expected
    );
}

#[test]
fn failure_in_one_cache_family_preserves_only_that_family_for_retry() {
    let available = Arc::new(AtomicBool::new(false));
    let can_write = available.clone();
    let saved = Arc::new(Mutex::new(BTreeMap::new()));
    let successes = saved.clone();
    let writer = ServerCacheWriter::start_with(move |server, snapshot| {
        if snapshot.kind() == CacheKind::ListeningHistory && !can_write.load(Ordering::SeqCst) {
            anyhow::bail!("history write unavailable");
        }
        *successes
            .lock()
            .unwrap()
            .entry(CacheKey::new(server, snapshot.kind()))
            .or_insert(0) += 1;
        Ok(())
    })
    .unwrap();
    for snapshot in snapshots("one", 2) {
        writer.submit(Some("one"), snapshot).unwrap();
    }
    assert!(writer
        .flush()
        .unwrap_err()
        .contains("history write unavailable"));
    assert!(writer.pending_session(Some("one")).is_none());
    assert!(writer.pending_loudness(Some("one")).is_none());
    assert!(writer.pending_genres(Some("one")).is_none());
    assert!(writer.pending_daily_mix(Some("one")).is_none());
    assert!(writer.pending_history(Some("one")).unwrap().events[0].completed);
    available.store(true, Ordering::SeqCst);
    assert_eq!(writer.flush(), Ok(()));
    assert_eq!(saved.lock().unwrap().len(), 5);
    assert!(saved.lock().unwrap().values().all(|count| *count == 1));
}

#[test]
fn discarding_a_server_cancels_all_cache_families_and_preserves_other_servers() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let saved = calls.clone();
    let mut first = true;
    let writer = Arc::new(
        ServerCacheWriter::start_with(move |server, snapshot| {
            if first {
                first = false;
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            saved
                .lock()
                .unwrap()
                .push(CacheKey::new(server, snapshot.kind()));
            Ok(())
        })
        .unwrap(),
    );
    writer.submit(Some("one"), session(0.0)).unwrap();
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    for snapshot in snapshots("one", 2) {
        writer.submit(Some("one"), snapshot).unwrap();
    }
    writer
        .submit(
            Some("two"),
            CacheSnapshot::ListeningHistory(ListeningHistory::default()),
        )
        .unwrap();
    let cancel = writer.clone();
    let discard = thread::spawn(move || cancel.discard(Some("one")));
    let deadline = Instant::now() + Duration::from_secs(5);
    while writer.pending_session(Some("one")).is_some() {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert!(writer.pending_loudness(Some("one")).is_none());
    assert!(writer.pending_genres(Some("one")).is_none());
    assert!(writer.pending_history(Some("one")).is_none());
    assert!(writer.pending_daily_mix(Some("one")).is_none());
    assert!(writer.pending_history(Some("two")).is_some());
    release_tx.send(()).unwrap();
    discard.join().unwrap();
    assert_eq!(writer.flush(), Ok(()));
    assert_eq!(
        *calls.lock().unwrap(),
        [
            CacheKey::new(Some("one"), CacheKind::Session),
            CacheKey::new(Some("two"), CacheKind::ListeningHistory)
        ]
    );
}

#[test]
fn blocked_writes_keep_only_the_latest_snapshot_per_server_and_flush_waits() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let saved = calls.clone();
    let mut first = true;
    let writer = Arc::new(
        session_writer(move |server, session| {
            if first {
                first = false;
                entered_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            saved
                .lock()
                .unwrap()
                .push((server.unwrap().to_string(), session.position_secs));
            Ok(())
        })
        .unwrap(),
    );
    writer.submit(Some("one"), session(1.0)).unwrap();
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    for position in 2..=50 {
        writer
            .submit(Some("one"), session(position as f64))
            .unwrap();
    }
    writer.submit(Some("two"), session(100.0)).unwrap();
    assert_eq!(writer.mailbox.pending_count(), 2);
    assert_eq!(
        writer.pending_session(Some("one")).unwrap().position_secs,
        50.0
    );
    let flush_writer = writer.clone();
    let (flushed_tx, flushed_rx) = mpsc::channel();
    let flush = thread::spawn(move || flushed_tx.send(flush_writer.flush()).unwrap());
    assert!(matches!(
        flushed_rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    release_tx.send(()).unwrap();
    assert_eq!(
        flushed_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        Ok(())
    );
    flush.join().unwrap();
    assert_eq!(
        *calls.lock().unwrap(),
        [
            ("one".into(), 1.0),
            ("one".into(), 50.0),
            ("two".into(), 100.0)
        ]
    );
    assert!(writer.pending_session(Some("one")).is_none());
    assert!(writer.pending_session(Some("two")).is_none());
}

#[test]
fn a_failed_clear_is_retained_and_flush_retries_it_without_restoring_old_songs() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("session.json");
    super::super::json::write_json_atomic(&destination, &session(42.0), "fixture session").unwrap();
    let attempts = Arc::new(AtomicUsize::new(0));
    let writes = attempts.clone();
    let available = Arc::new(AtomicBool::new(false));
    let can_write = available.clone();
    let path = destination.clone();
    let writer = session_writer(move |_, session| {
        writes.fetch_add(1, Ordering::SeqCst);
        if !can_write.load(Ordering::SeqCst) {
            anyhow::bail!("synthetic save failure");
        }
        super::super::json::write_json_atomic(&path, session, "fixture session")
    })
    .unwrap();
    let empty = PlaybackSession {
        server_url: "http://fixture.test".into(),
        ..Default::default()
    };
    writer.submit(None, empty).unwrap();
    let error = writer.flush().unwrap_err();
    assert!(error.contains("synthetic save failure"));
    let before: PlaybackSession = super::super::json::read_json_at(&destination).unwrap();
    assert_eq!(before.queue[0].id, "song");
    assert!(writer.pending_session(None).unwrap().queue.is_empty());
    let previous_attempts = attempts.load(Ordering::SeqCst);
    available.store(true, Ordering::SeqCst);
    assert_eq!(writer.flush(), Ok(()));
    assert_eq!(attempts.load(Ordering::SeqCst), previous_attempts + 1);
    let after: PlaybackSession = super::super::json::read_json_at(&destination).unwrap();
    assert!(after.queue.is_empty());
    assert!(writer.pending_session(None).is_none());
}

#[test]
fn failed_writes_wait_for_retry_deadline_and_a_new_snapshot_supersedes_them() {
    let writer = session_writer(|_, _| anyhow::bail!("offline filesystem")).unwrap();
    writer.submit(Some("one"), session(10.0)).unwrap();
    assert!(writer.flush().is_err());
    assert!(writer.mailbox.retry_at(Some("one"), CacheKind::Session) > Instant::now());
    writer.submit(Some("one"), session(30.0)).unwrap();
    assert!(writer.flush().is_err());
    assert_eq!(
        writer.pending_session(Some("one")).unwrap().position_secs,
        30.0
    );
    writer.discard(Some("one"));
    assert_eq!(writer.flush(), Ok(()));
}

#[test]
fn retry_deadline_saves_a_failed_snapshot_without_another_submission_or_flush() {
    let available = Arc::new(AtomicBool::new(false));
    let can_write = available.clone();
    let (saved_tx, saved_rx) = mpsc::channel();
    let writer = session_writer(move |_, session| {
        if !can_write.load(Ordering::SeqCst) {
            anyhow::bail!("filesystem unavailable");
        }
        saved_tx.send(session.position_secs).unwrap();
        Ok(())
    })
    .unwrap();
    writer.submit(Some("one"), session(42.125)).unwrap();
    assert!(writer.flush().is_err());
    available.store(true, Ordering::SeqCst);
    // Give only this fixture an imminent retry deadline. The real scheduler
    // wakes on its timeout and saves without a new app request.
    writer.mailbox.set_retry_at(
        Some("one"),
        CacheKind::Session,
        Instant::now() + Duration::from_millis(10),
    );
    assert_eq!(
        saved_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        42.125
    );
    assert_eq!(writer.flush(), Ok(()));
    assert!(writer.pending_session(Some("one")).is_none());
}

#[test]
fn discarding_a_server_waits_for_its_running_write_and_cancels_retries() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("session-one.json");
    std::fs::create_dir(&destination).unwrap();
    let path = destination.clone();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let writer = Arc::new(
        session_writer(move |_, session| {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            super::super::json::write_json_atomic(&path, session, "fixture session")
        })
        .unwrap(),
    );
    writer.submit(Some("one"), session(10.0)).unwrap();
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    writer.submit(Some("one"), session(20.0)).unwrap();
    let cancel = writer.clone();
    let (done_tx, done_rx) = mpsc::channel();
    let discard = thread::spawn(move || {
        cancel.discard(Some("one"));
        done_tx.send(()).unwrap();
    });
    // Wait until discard has actually removed the waiting replacement before
    // allowing the in-flight save to fail. No wall-clock sleep is needed.
    let deadline = Instant::now() + Duration::from_secs(5);
    while writer.pending_session(Some("one")).is_some() {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert!(matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    release_tx.send(()).unwrap();
    done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    discard.join().unwrap();
    assert!(writer.pending_session(Some("one")).is_none());
    assert_eq!(writer.flush(), Ok(()));
    std::fs::remove_dir(&destination).unwrap();
    drop(writer);
    assert!(!destination.exists());
}

#[test]
fn discard_releases_a_flush_waiter_for_a_cancelled_snapshot() {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let writer = Arc::new(
        session_writer(move |_, _| {
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            anyhow::bail!("cancelled write");
        })
        .unwrap(),
    );
    writer.submit(Some("one"), session(10.0)).unwrap();
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let old_deadline = Instant::now() - Duration::from_secs(60);
    writer
        .mailbox
        .set_retry_at(Some("one"), CacheKind::Session, old_deadline);
    let flush_writer = writer.clone();
    let (flushed_tx, flushed_rx) = mpsc::channel();
    let flush = thread::spawn(move || flushed_tx.send(flush_writer.flush()).unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    while writer.mailbox.retry_at(Some("one"), CacheKind::Session) == old_deadline {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    let discard_writer = writer.clone();
    let discard = thread::spawn(move || discard_writer.discard(Some("one")));
    let result = flushed_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(result.unwrap_err().contains("discarded"));
    release_tx.send(()).unwrap();
    discard.join().unwrap();
    flush.join().unwrap();
    assert_eq!(writer.flush(), Ok(()));
}

#[test]
fn writer_shutdown_finishes_a_submitted_snapshot_and_joins_its_worker() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.json");
    let destination = path.clone();
    let writer = session_writer(move |_, session| {
        super::super::json::write_json_atomic(&destination, session, "fixture session")
    })
    .unwrap();
    writer.submit(Some("one"), session(42.125)).unwrap();
    drop(writer);
    let saved: PlaybackSession = super::super::json::read_json_at(&path).unwrap();
    assert_eq!(saved.position_secs, 42.125);
    assert_eq!(saved.queue_origins, [QueueOrigin::Playlist]);
}

#[test]
fn a_panicking_worker_releases_flush_waiters_and_rejects_new_work() {
    let writer = session_writer(|_, _| panic!("synthetic worker exit")).unwrap();
    writer.submit(Some("one"), session(10.0)).unwrap();
    assert!(writer.flush().is_err());
    assert!(writer.submit(Some("one"), session(20.0)).is_err());
    assert_eq!(
        writer.pending_session(Some("one")).unwrap().position_secs,
        10.0
    );
}
