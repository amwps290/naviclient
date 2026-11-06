use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use super::{NotificationWorker, TrackNotification};

const DEADLINE: Duration = Duration::from_secs(5);

struct ExitSignal(Sender<()>);

impl Drop for ExitSignal {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}

/// Each fake native call blocks until the test supplies a result. This proves
/// queue/drop behavior with bounded channel waits and no notification server.
struct Harness {
    worker: Option<NotificationWorker>,
    attempts: Receiver<(TrackNotification, Option<u32>)>,
    results: Sender<anyhow::Result<u32>>,
    exited: Receiver<()>,
}

impl Harness {
    fn new() -> Self {
        let (attempts_tx, attempts) = mpsc::channel();
        let (results, results_rx) = mpsc::channel();
        let (exited_tx, exited) = mpsc::channel();
        let exit_signal = ExitSignal(exited_tx);
        let worker = NotificationWorker::start(move |notification, id| {
            let _keep_alive = &exit_signal;
            attempts_tx.send((notification.clone(), id))?;
            results_rx.recv_timeout(DEADLINE)?
        })
        .unwrap();
        Self {
            worker: Some(worker),
            attempts,
            results,
            exited,
        }
    }

    fn submit(&self, title: &str) {
        self.worker.as_ref().unwrap().submit(notification(title));
    }

    fn next(&self) -> (TrackNotification, Option<u32>) {
        self.attempts.recv_timeout(DEADLINE).unwrap()
    }

    fn succeed(&self, id: u32) {
        self.results.send(Ok(id)).unwrap();
    }

    fn expect_no_attempt(&self) {
        assert!(matches!(
            self.attempts.recv_timeout(Duration::from_millis(100)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
    }

    fn wait_for_exit(&self) {
        self.exited.recv_timeout(DEADLINE).unwrap();
        assert!(matches!(
            self.attempts.recv_timeout(DEADLINE),
            Err(mpsc::RecvTimeoutError::Disconnected)
        ));
    }

    fn stop(&mut self) {
        self.worker.take();
        self.wait_for_exit();
    }
}

fn notification(title: &str) -> TrackNotification {
    TrackNotification {
        title: title.into(),
        body: format!("artist — {title}"),
        cover_path: Some(format!("covers/{title}.png").into()),
    }
}

#[test]
fn blocked_sender_does_not_block_submissions_and_only_sends_latest_pending_track() {
    let mut harness = Harness::new();
    harness.submit("first");
    assert_eq!(harness.next(), (notification("first"), None));
    for index in 0..100 {
        harness.submit(&format!("skipped-{index}"));
    }
    harness.submit("latest");
    harness.succeed(10);
    assert_eq!(harness.next(), (notification("latest"), Some(10)));
    harness.succeed(11);
    harness.stop();
}

#[test]
fn successful_sends_use_the_latest_native_id_and_failure_clears_it() {
    let mut harness = Harness::new();
    harness.submit("first");
    assert_eq!(harness.next().1, None);
    harness.submit("second");
    harness.succeed(10);
    assert_eq!(harness.next().1, Some(10));
    harness.submit("failed");
    harness.succeed(20);
    assert_eq!(harness.next().1, Some(20));
    harness.submit("recovered");
    harness
        .results
        .send(Err(anyhow::anyhow!("fake notification failure")))
        .unwrap();
    assert_eq!(harness.next(), (notification("recovered"), None));
    harness.succeed(30);
    harness.stop();
}

#[test]
fn cancellation_discards_queued_tracks_and_retains_native_replacement_id() {
    let mut harness = Harness::new();
    harness.submit("in-flight");
    assert_eq!(harness.next().1, None);
    harness.submit("cancelled");
    harness.worker.as_ref().unwrap().cancel_pending();
    harness.succeed(10);
    harness.expect_no_attempt();
    harness.submit("new-server");
    assert_eq!(harness.next(), (notification("new-server"), Some(10)));
    harness.succeed(11);
    harness.stop();
}

#[test]
fn cancellation_without_replacement_discards_queued_work() {
    let mut harness = Harness::new();
    harness.submit("in-flight");
    harness.next();
    harness.submit("cancelled");
    harness.worker.as_ref().unwrap().cancel_pending();
    harness.worker.as_ref().unwrap().cancel_pending();
    harness.succeed(10);
    harness.expect_no_attempt();
    harness.stop();
}

#[test]
fn drop_returns_during_a_blocked_native_call_and_discards_pending_work() {
    let mut harness = Harness::new();
    harness.submit("in-flight");
    harness.next();
    harness.submit("discarded");
    harness.worker.take();
    // If Drop joined the sender, this could not release the blocked native call.
    harness.succeed(10);
    harness.wait_for_exit();
}

#[test]
fn dropping_an_idle_worker_wakes_it_and_releases_the_backend() {
    let mut harness = Harness::new();
    harness.stop();
}
