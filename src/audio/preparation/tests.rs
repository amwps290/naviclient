use super::super::error::classify_anyhow;
use super::*;
use crate::audio::buffered_stream::{BufferedStreamSource, StreamMessage};
use crate::audio::preparation_state::{PreparationKind, PreparationState};
use rodio::{ChannelCount, SampleRate};

#[test]
fn prepared_value_is_delivered_once_with_its_request_identity() {
    let mut state = PreparationState::default();
    let token = state.begin(
        PreparationKind::Seek,
        Duration::from_millis(30_125),
        Instant::now(),
    );
    let (tx, rx) = mpsc::channel();
    spawn_with(
        tx,
        token,
        StreamControl::new(false),
        |_| Ok(23),
        |task| {
            task();
            Ok(())
        },
    );
    let result = rx.recv().unwrap();
    assert_eq!(result.token, token);
    assert_eq!(result.result.unwrap(), 23);
    assert!(state.complete(result.token));
    assert!(matches!(
        rx.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
}

#[test]
fn preparation_failure_retains_classification_and_ends_the_wait() {
    let mut state = PreparationState::default();
    let token = state.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
    let (tx, rx) = mpsc::channel();
    spawn_with::<()>(
        tx,
        token,
        StreamControl::new(false),
        |_| {
            Err(anyhow!(FailureError::new(
                StreamFailure::Timeout,
                "stream timed out"
            )))
        },
        |task| {
            task();
            Ok(())
        },
    );
    let result = rx.recv().unwrap();
    let error = result.result.unwrap_err();
    assert_eq!(classify_anyhow(&error), StreamFailure::Timeout);
    assert!(state.complete(result.token));
    assert!(!state.preparing());
}

#[test]
fn failed_thread_creation_reports_error_instead_of_leaving_buffering_active() {
    for kind in [
        io::ErrorKind::WouldBlock,
        io::ErrorKind::Other,
        io::ErrorKind::PermissionDenied,
    ] {
        let mut state = PreparationState::default();
        let token = state.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
        let (tx, rx) = mpsc::channel();
        spawn_with::<()>(
            tx,
            token,
            StreamControl::new(false),
            |_| panic!("preparation must not run when thread creation fails"),
            |_| Err(io::Error::new(kind, "thread capacity exhausted")),
        );
        let result = rx.recv().unwrap();
        assert_eq!(result.token, token);
        let error = result.result.unwrap_err();
        let failure = classify_anyhow(&error);
        assert_eq!(failure, StreamFailure::Preparation);
        assert!(!failure.is_retryable());
        let error = format!("{error:#}");
        assert!(error.contains("failed to start the audio preparation thread"));
        assert!(error.contains("thread capacity exhausted"));
        assert!(state.complete(result.token));
        assert!(!state.preparing());
        assert_eq!(state.elapsed(Instant::now()), None);
        assert!(matches!(
            rx.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }
}

#[test]
fn stale_spawn_failure_does_not_clear_a_new_seek_or_its_pause_intent() {
    let now = Instant::now();
    let mut state = PreparationState::default();
    let old = state.begin(PreparationKind::Play, Duration::ZERO, now);
    let current = state.begin(PreparationKind::Seek, Duration::from_secs(30), now);
    state.set_paused(true);
    let (tx, rx) = mpsc::channel();
    spawn_with::<()>(
        tx,
        old,
        StreamControl::new(false),
        |_| Ok(()),
        |_| Err(io::Error::other("spawn rejected")),
    );
    let result = rx.recv().unwrap();
    assert!(result.result.is_err());
    assert!(!state.complete(result.token));
    assert!(state.preparing() && state.paused());
    assert!(state.complete(current));
}

#[test]
fn blocked_preparation_finishing_after_stop_and_new_play_cannot_replace_current_request() {
    let mut state = PreparationState::default();
    let old = state.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
    let (tx, rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let mut worker = None;
    spawn_with(
        tx,
        old,
        StreamControl::new(false),
        move |_| {
            started_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(7)
        },
        |task| {
            worker = Some(thread::spawn(task));
            Ok(())
        },
    );
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    state.stop();
    let current = state.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
    state.set_paused(true);
    release_tx.send(()).unwrap();
    worker.unwrap().join().unwrap();
    let result = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(result.result.unwrap(), 7);
    assert!(!state.complete(result.token));
    assert!(state.preparing() && state.paused());
    assert!(state.complete(current));
}

fn inline(task: Box<dyn FnOnce() + Send>) -> io::Result<()> {
    task();
    Ok(())
}

#[test]
fn replacing_play_with_seek_cancels_old_work_and_preserves_pause_intent() {
    let now = Instant::now();
    let mut preparation = Preparation::<()>::new();
    preparation.begin(PreparationKind::Play, Duration::ZERO, now);
    let old_control = preparation.pending.as_ref().unwrap().control.clone();
    preparation.set_paused(true);
    let seek = preparation.begin(PreparationKind::Seek, Duration::from_secs(30), now);
    assert!(old_control.check().is_err());
    assert!(preparation.preparing() && preparation.paused());
    assert_eq!(preparation.state().elapsed(now), Some(Duration::ZERO));
    preparation.deliver(seek, Ok(()));
    assert_eq!(preparation.poll().unwrap().token, seek);
    assert!(!preparation.preparing());
    assert!(preparation.paused());
}

#[test]
fn stopped_or_replaced_task_cannot_start_after_thread_scheduling() {
    for replace in [false, true] {
        let mut preparation = Preparation::<()>::new();
        let old = preparation.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
        let mut scheduled = None;
        preparation.spawn_with(
            old,
            |_| panic!("cancelled task must not start network or decoder work"),
            |task| {
                scheduled = Some(task);
                Ok(())
            },
        );
        preparation.stop();
        assert!(!preparation.preparing() && !preparation.paused());
        let current = replace
            .then(|| preparation.begin(PreparationKind::Play, Duration::ZERO, Instant::now()));
        scheduled.unwrap()();
        assert!(preparation.poll().is_none());
        assert_eq!(preparation.preparing(), replace);
        if let Some(current) = current {
            preparation.deliver(current, Ok(()));
            assert!(preparation.poll().unwrap().result.is_ok());
        }
    }
}

#[test]
fn stale_or_duplicate_start_never_launches_another_task() {
    let mut preparation = Preparation::<u32>::new();
    let old = preparation.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
    let current = preparation.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
    preparation.spawn_with(old, |_| panic!("stale task"), |_| panic!("stale spawn"));
    preparation.spawn_with(current, |_| Ok(23), inline);
    preparation.spawn_with(
        current,
        |_| panic!("duplicate task"),
        |_| panic!("duplicate spawn"),
    );
    preparation.deliver(current, Ok(99));
    assert_eq!(preparation.poll().unwrap().result.unwrap(), 23);
    assert!(preparation.poll().is_none());
}

#[test]
fn current_startup_error_is_delivered_even_when_startup_cancelled_its_control() {
    let mut preparation = Preparation::<()>::new();
    let token = preparation.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
    preparation.spawn_with(
        token,
        |control| {
            control.cancel();
            Err(anyhow!(FailureError::new(
                StreamFailure::Decode,
                "empty audio"
            )))
        },
        inline,
    );
    let completion = preparation.poll().unwrap();
    assert_eq!(completion.token, token);
    assert_eq!(
        classify_anyhow(&completion.result.unwrap_err()),
        StreamFailure::Decode
    );
    assert!(!preparation.preparing());
    assert!(preparation.poll().is_none());
}

#[test]
fn coordinator_spawn_failure_ends_wait_and_retains_pause_intent() {
    let mut preparation = Preparation::<()>::new();
    let token = preparation.begin(
        PreparationKind::Seek,
        Duration::from_secs(30),
        Instant::now(),
    );
    preparation.set_paused(true);
    preparation.spawn_with(
        token,
        |_| panic!("failed spawn cannot run preparation"),
        |_| Err(io::Error::other("thread capacity exhausted")),
    );
    let completion = preparation.poll().unwrap();
    assert_eq!(
        classify_anyhow(&completion.result.unwrap_err()),
        StreamFailure::Preparation
    );
    assert!(!preparation.preparing());
    assert!(preparation.paused());
}

#[test]
fn late_old_error_does_not_consume_the_current_result() {
    let mut preparation = Preparation::<u32>::new();
    let old = preparation.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
    let sender = preparation.sender.clone();
    let current = preparation.begin(
        PreparationKind::Seek,
        Duration::from_secs(30),
        Instant::now(),
    );
    preparation.set_paused(true);
    sender
        .send(PreparationResult {
            token: old,
            result: Err(anyhow!("cancelled read eventually returned")),
        })
        .unwrap();
    assert!(preparation.poll().is_none());
    assert!(preparation.preparing() && preparation.paused());
    preparation.deliver(current, Ok(23));
    assert_eq!(preparation.poll().unwrap().result.unwrap(), 23);
    assert!(preparation.paused());
}

fn prepared_source() -> BufferedStreamSource {
    super::super::buffered_stream::tests::from_samples(
        vec![0.25, 0.25],
        ChannelCount::new(2).unwrap(),
        SampleRate::new(48_000).unwrap(),
        None,
    )
    .0
}

#[test]
fn stop_replacement_and_drop_release_queued_prepared_sources() {
    for action in 0..3 {
        let mut preparation = Preparation::new();
        let token = preparation.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
        let source = prepared_source();
        let control = source.control().clone();
        preparation.deliver(token, Ok(source));
        match action {
            0 => preparation.stop(),
            1 => {
                preparation.begin(PreparationKind::Seek, Duration::ZERO, Instant::now());
            }
            _ => drop(preparation),
        }
        assert!(control.check().is_err(), "queued decoder must be cancelled");
    }
}

#[test]
fn late_stale_success_releases_its_source_without_clearing_current_wait() {
    let mut preparation = Preparation::new();
    let old = preparation.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
    preparation.begin(
        PreparationKind::Seek,
        Duration::from_secs(30),
        Instant::now(),
    );
    let source = prepared_source();
    let control = source.control().clone();
    preparation
        .sender
        .send(PreparationResult {
            token: old,
            result: Ok(source),
        })
        .unwrap();
    assert!(preparation.poll().is_none());
    assert!(control.check().is_err());
    assert!(preparation.preparing());
}

#[test]
fn accepted_prepared_source_remains_playable_after_coordinator_is_dropped() {
    let mut preparation = Preparation::new();
    let token = preparation.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
    preparation.deliver(token, Ok(prepared_source()));
    let mut source = preparation.poll().unwrap().result.unwrap();
    let control = source.control().clone();
    drop(preparation);
    assert!(control.check().is_ok());
    assert_eq!(source.next(), Some(0.25));
    assert_eq!(source.next(), Some(0.25));
    assert_eq!(source.next(), None);
    drop(source);
    assert!(control.check().is_err());
}

#[test]
fn accepted_spawned_source_transfers_its_actual_cancellation_handle_to_playback() {
    let mut preparation = Preparation::new();
    let token = preparation.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
    let control = preparation.pending.as_ref().unwrap().control.clone();
    preparation.spawn_with(
        token,
        |control| {
            let (sender, receiver) = mpsc::channel();
            sender
                .send(StreamMessage::Samples(vec![0.25, 0.25]))
                .unwrap();
            sender.send(StreamMessage::End).unwrap();
            BufferedStreamSource::new(
                receiver,
                None,
                ChannelCount::new(2).unwrap(),
                SampleRate::new(48_000).unwrap(),
                control,
            )
            .map(|(source, _)| source)
        },
        inline,
    );
    let mut source = preparation.poll().unwrap().result.unwrap();
    assert!(Arc::ptr_eq(source.control(), &control));
    preparation.stop();
    drop(preparation);
    assert!(control.check().is_ok());
    assert_eq!(source.next(), Some(0.25));
    drop(source);
    assert!(control.check().is_err());
}

#[test]
fn direct_output_handoff_relinquishes_pending_control_without_cancelling_it() {
    let mut preparation = Preparation::<()>::new();
    let token = preparation.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
    let control = preparation.pending.as_ref().unwrap().control.clone();
    assert!(preparation.complete(token));
    assert!(!preparation.complete(token));
    preparation.stop();
    drop(preparation);
    assert!(control.check().is_ok());
}

#[test]
fn blocked_old_preparation_does_not_hold_up_new_play_and_drops_late_success() {
    let mut preparation = Preparation::new();
    let old = preparation.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let source = prepared_source();
    let source_control = source.control().clone();
    let mut worker = None;
    preparation.spawn_with(
        old,
        move |_| {
            started_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(source)
        },
        |task| {
            worker = Some(thread::spawn(task));
            Ok(())
        },
    );
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let current = preparation.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
    preparation.deliver(current, Ok(prepared_source()));
    let accepted = preparation.poll().unwrap();
    assert_eq!(accepted.token, current);
    release_tx.send(()).unwrap();
    worker.unwrap().join().unwrap();
    assert!(source_control.check().is_err());
    assert!(preparation.poll().is_none());
    assert!(!preparation.preparing());
}

#[test]
fn dropping_coordinator_cancels_a_blocked_task_and_closes_completion_delivery() {
    let mut preparation = Preparation::<()>::new();
    let token = preparation.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
    let control = preparation.pending.as_ref().unwrap().control.clone();
    let sender = preparation.sender.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let mut worker = None;
    preparation.spawn_with(
        token,
        move |control| {
            started_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(control.check().is_err());
            Err(anyhow!("cancelled read"))
        },
        |task| {
            worker = Some(thread::spawn(task));
            Ok(())
        },
    );
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(preparation);
    assert!(control.check().is_err());
    assert!(sender
        .send(PreparationResult {
            token,
            result: Ok(())
        })
        .is_err());
    release_tx.send(()).unwrap();
    worker.unwrap().join().unwrap();
}
