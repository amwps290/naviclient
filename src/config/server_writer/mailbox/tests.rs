use super::*;
use crate::models::PlaybackSession;
use std::sync::mpsc;
use std::thread;

fn session(position: f64) -> CacheSnapshot {
    PlaybackSession {
        position_secs: position,
        ..Default::default()
    }
    .into()
}

fn position(snapshot: &CacheSnapshot) -> f64 {
    let CacheSnapshot::Session(session) = snapshot else {
        panic!("expected session snapshot")
    };
    session.position_secs
}

#[test]
fn pending_reads_and_attempts_share_owned_snapshots_across_replacement_and_completion() {
    let mailbox = Mailbox::default();
    mailbox.submit(Some("One"), session(10.0)).unwrap();
    let snapshot = mailbox.snapshot(Some("ONE/"), CacheKind::Session).unwrap();
    let task = mailbox.next().unwrap();
    assert_eq!(task.server_id(), Some("One"));
    assert!(Arc::ptr_eq(&snapshot, &task.snapshot));

    mailbox.submit(Some("one/"), session(20.0)).unwrap();
    let replacement = mailbox.snapshot(Some("one"), CacheKind::Session).unwrap();
    assert!(!Arc::ptr_eq(&snapshot, &replacement));
    assert_eq!(position(task.snapshot()), 10.0);
    mailbox.complete(task, None);
    let task = mailbox.next().unwrap();
    assert_eq!(task.server_id(), Some("one/"));
    assert!(Arc::ptr_eq(&replacement, &task.snapshot));
    mailbox.complete(task, None);

    assert!(mailbox.snapshot(Some("one"), CacheKind::Session).is_none());
    assert_eq!(position(&snapshot), 10.0);
    assert_eq!(position(&replacement), 20.0);
}

#[test]
fn superseded_completion_neither_removes_nor_delays_the_latest_snapshot() {
    for error in [None, Some("old write failed".into())] {
        let mailbox = Mailbox::default();
        mailbox.submit(Some("one"), session(10.0)).unwrap();
        let old = mailbox.next().unwrap();
        mailbox.submit(Some("ONE/"), session(20.0)).unwrap();
        let retry_at = mailbox.retry_at(Some("one"), CacheKind::Session);

        mailbox.complete(old, error);
        assert_eq!(mailbox.pending_count(), 1);
        assert_eq!(mailbox.retry_at(Some("one"), CacheKind::Session), retry_at);
        let task = mailbox.next().unwrap();
        assert_eq!(position(task.snapshot()), 20.0);
        mailbox.complete(task, None);
        assert_eq!(mailbox.flush(), Ok(()));
    }
}

#[test]
fn next_orders_attempts_by_retry_deadline_then_revision_instead_of_server_name() {
    let mailbox = Mailbox::default();
    for (server, value) in [("z", 1.0), ("m", 2.0), ("a", 3.0)] {
        mailbox.submit(Some(server), session(value)).unwrap();
    }
    let due = Instant::now();
    mailbox.set_retry_at(Some("z"), CacheKind::Session, due + RETRY_INTERVAL);
    for server in ["m", "a"] {
        mailbox.set_retry_at(Some(server), CacheKind::Session, due);
    }
    for server in ["m", "a"] {
        let task = mailbox.next().unwrap();
        assert_eq!(task.server_id(), Some(server));
        mailbox.complete(task, None);
    }
    assert_eq!(mailbox.pending_count(), 1);
    mailbox.set_retry_at(Some("z"), CacheKind::Session, due);
    let task = mailbox.next().unwrap();
    assert_eq!(task.server_id(), Some("z"));
    mailbox.complete(task, None);
    assert_eq!(mailbox.pending_count(), 0);
}

#[test]
fn flush_waits_for_a_fresh_attempt_of_the_same_revision_after_a_failure() {
    let mailbox = Arc::new(Mailbox::default());
    mailbox.submit(None, session(10.0)).unwrap();
    let task = mailbox.next().unwrap();
    mailbox.complete(task, Some("first attempt failed".into()));
    let old_deadline = Instant::now() - Duration::from_secs(60);
    mailbox.set_retry_at(None, CacheKind::Session, old_deadline);

    let waiter = mailbox.clone();
    let (done_tx, done_rx) = mpsc::channel();
    let flush = thread::spawn(move || done_tx.send(waiter.flush()).unwrap());
    let deadline = Instant::now() + Duration::from_secs(5);
    while mailbox.retry_at(None, CacheKind::Session) == old_deadline {
        assert!(Instant::now() < deadline);
        thread::yield_now();
    }
    assert!(matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    let task = mailbox.next().unwrap();
    assert_eq!(position(task.snapshot()), 10.0);
    mailbox.complete(task, None);
    assert_eq!(
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        Ok(())
    );
    flush.join().unwrap();
}

#[test]
fn worker_closure_and_stop_reject_new_work_without_discarding_unsaved_snapshots() {
    for stopped in [false, true] {
        let mailbox = Arc::new(Mailbox::default());
        let guard = mailbox.worker_guard();
        mailbox.submit(None, session(10.0)).unwrap();
        if stopped {
            mailbox.stop();
            assert!(mailbox.next().is_none());
        } else {
            let task = mailbox.next().unwrap();
            drop(guard);
            assert!(mailbox.flush().unwrap_err().contains("closed"));
            assert!(mailbox.state.lock().unwrap().in_flight.is_none());
            drop(task);
        }
        assert!(mailbox.submit(None, session(20.0)).is_err());
        assert_eq!(
            position(&mailbox.snapshot(None, CacheKind::Session).unwrap()),
            10.0
        );
        mailbox.discard(None);
        assert_eq!(mailbox.flush(), Ok(()));
    }
}
