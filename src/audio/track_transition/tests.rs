use super::*;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{mpsc, Barrier};
use std::thread;

fn reading(value: Option<(f32, f32)>) -> Loudness {
    Arc::new(Mutex::new(value))
}

fn fingerprint(event: TrackTransition) -> (u64, Duration, u32, u32) {
    (
        event.id,
        event.boundary,
        event.loudness_db.to_bits(),
        event.loudness_peak.to_bits(),
    )
}

#[test]
fn pending_observations_do_not_consume_a_future_event_and_cancellation_is_permanent() {
    for duration in [None, Some(Duration::from_nanos(123))] {
        let transition = Transition::new(7, duration);
        assert_eq!(transition.id, 7);
        assert_eq!(transition.duration, duration);
        for _ in 0..3 {
            assert!(transition.pending());
            assert!(transition.event().is_none());
            assert!(transition.take_event().is_none());
        }
        assert!(transition.start(Duration::ZERO, &reading(None)));
        assert_eq!(transition.take_event().unwrap().id, 7);
        assert!(transition.take_event().is_none());

        let cancelled = Transition::new(8, duration);
        cancelled.cancel();
        cancelled.cancel();
        assert!(!cancelled.pending());
        assert!(!cancelled.start(Duration::ZERO, &reading(None)));
        assert!(cancelled.event().is_none());
        assert!(cancelled.take_event().is_none());
    }
}

#[test]
fn started_metadata_is_immutable_and_remains_observable_after_cancellation_and_consumption() {
    for values in [
        (-8.0, 0.8),
        (f32::NAN, f32::INFINITY),
        (f32::NEG_INFINITY, -0.0),
    ] {
        let previous = reading(Some(values));
        let transition = Transition::new(9, Some(Duration::from_secs(10)));
        let boundary = Duration::from_nanos(123_456_789);
        assert!(transition.start(boundary, &previous));
        let expected = (9, boundary, values.0.to_bits(), values.1.to_bits());
        *previous.lock().unwrap() = Some((-1.0, 0.1));
        transition.cancel();
        assert!(!transition.pending());
        assert!(!transition.start(Duration::from_secs(99), &previous));
        assert_eq!(fingerprint(transition.event().unwrap()), expected);
        assert_eq!(fingerprint(transition.take_event().unwrap()), expected);
        assert!(transition.take_event().is_none());
        assert_eq!(fingerprint(transition.event().unwrap()), expected);
    }
}

#[test]
fn missing_or_poisoned_loudness_publishes_zero_without_preventing_the_boundary() {
    for poison in [false, true] {
        let previous = reading(None);
        if poison {
            assert!(catch_unwind(AssertUnwindSafe(|| {
                let _guard = previous.lock().unwrap();
                panic!("injected previous-track loudness failure");
            }))
            .is_err());
        }
        let transition = Transition::new(3, None);
        assert!(transition.start(Duration::from_secs(2), &previous));
        assert_eq!(
            fingerprint(transition.take_event().unwrap()),
            (
                3,
                Duration::from_secs(2),
                0.0_f32.to_bits(),
                0.0_f32.to_bits()
            )
        );
    }
}

#[test]
fn concurrent_event_consumers_report_exactly_once_and_observation_is_non_consuming() {
    let transition = Transition::new(12, None);
    assert!(transition.start(Duration::from_secs(5), &reading(Some((-6.0, 0.5)))));
    let expected = fingerprint(transition.event().unwrap());
    let barrier = Arc::new(Barrier::new(17));
    let workers: Vec<_> = (0..16)
        .map(|_| {
            let transition = transition.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                assert_eq!(fingerprint(transition.event().unwrap()), expected);
                transition.take_event().map(fingerprint)
            })
        })
        .collect();
    barrier.wait();
    let events: Vec<_> = workers
        .into_iter()
        .filter_map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(events, vec![expected]);
    assert!(transition.take_event().is_none());
    assert_eq!(fingerprint(transition.event().unwrap()), expected);
}

#[test]
fn concurrent_start_attempts_retain_only_the_winners_boundary_and_loudness() {
    let transition = Transition::new(15, Some(Duration::from_secs(10)));
    let barrier = Arc::new(Barrier::new(9));
    let workers: Vec<_> = (0..8)
        .map(|index| {
            let transition = transition.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                let boundary = Duration::from_millis(index);
                let values = (-(index as f32), index as f32 / 10.0);
                let previous = reading(Some(values));
                barrier.wait();
                transition.start(boundary, &previous).then_some((
                    15,
                    boundary,
                    values.0.to_bits(),
                    values.1.to_bits(),
                ))
            })
        })
        .collect();
    barrier.wait();
    let winners: Vec<_> = workers
        .into_iter()
        .filter_map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(winners.len(), 1);
    assert_eq!(fingerprint(transition.event().unwrap()), winners[0]);
    assert_eq!(fingerprint(transition.take_event().unwrap()), winners[0]);
    assert!(transition.take_event().is_none());
}

#[test]
fn racing_cancel_and_start_leave_a_single_terminal_phase() {
    for id in 0..64 {
        let transition = Transition::new(id, None);
        let barrier = Arc::new(Barrier::new(2));
        let starter = transition.clone();
        let start_barrier = barrier.clone();
        let worker = thread::spawn(move || {
            start_barrier.wait();
            starter.start(Duration::from_secs(3), &reading(Some((-4.0, 0.75))))
        });
        barrier.wait();
        transition.cancel();
        let started = worker.join().unwrap();
        assert!(!transition.pending());
        assert_eq!(transition.event().is_some(), started);
        assert_eq!(transition.take_event().is_some(), started);
        assert!(transition.take_event().is_none());
        assert!(!transition.start(Duration::ZERO, &reading(None)));
        if started {
            assert_eq!(
                fingerprint(transition.event().unwrap()),
                (
                    id,
                    Duration::from_secs(3),
                    (-4.0_f32).to_bits(),
                    0.75_f32.to_bits()
                )
            );
        }
    }
}

#[test]
fn rejected_start_does_not_wait_for_previous_track_loudness() {
    for cancelled in [false, true] {
        let transition = Transition::new(1, None);
        if cancelled {
            transition.cancel();
        } else {
            assert!(transition.start(Duration::ZERO, &reading(None)));
        }
        let previous = reading(Some((-8.0, 0.8)));
        let guard = previous.lock().unwrap();
        let worker_previous = previous.clone();
        let (sender, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            sender
                .send(transition.start(Duration::from_secs(5), &worker_previous))
                .unwrap();
        });
        let early = receiver.recv_timeout(Duration::from_secs(5));
        drop(guard);
        worker.join().unwrap();
        assert!(
            !early.unwrap(),
            "a terminal transition must reject without measuring again"
        );
    }
}
