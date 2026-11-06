use super::*;
use crate::audio::error::StreamFailure;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;

const WAIT_TIMEOUT: Duration = Duration::from_secs(5);

// Release and join even if the waiter returns unexpectedly early. The startup
// handshake and bounded receive avoid treating thread creation as proof of a wait.
fn observe_wait<T: Send + 'static>(
    wait: impl FnOnce() -> T + Send + 'static,
    timeout: Duration,
    release: impl FnOnce(),
) -> (bool, T) {
    let (started_tx, started_rx) = mpsc::channel();
    let (finished_tx, finished_rx) = mpsc::channel();
    let worker = thread::spawn(move || {
        started_tx.send(()).unwrap();
        finished_tx.send(wait()).unwrap();
    });
    started_rx.recv_timeout(WAIT_TIMEOUT).unwrap();
    let early = finished_rx.recv_timeout(timeout);
    release();
    let completed_early = early.is_ok();
    let result = match early {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => finished_rx.recv_timeout(WAIT_TIMEOUT).unwrap(),
        Err(RecvTimeoutError::Disconnected) => {
            worker.join().unwrap();
            panic!("stream-control waiter disconnected without returning");
        }
    };
    worker.join().unwrap();
    (completed_early, result)
}

#[test]
fn published_completion_is_monotonic_and_keeps_creation_time_cache_ownership() {
    for speculative in [false, true] {
        let control = StreamControl::new(speculative);
        let observer = control.clone();
        assert!(!observer.fully_decoded());
        control.mark_fully_decoded();
        control.mark_fully_decoded();
        assert!(observer.fully_decoded());
        control.promote();
        assert_eq!(observer.is_speculative(), speculative);
        assert!(observer.fully_decoded());
        control.cancel();
        assert!(observer.check().is_err());
        assert!(observer.fully_decoded());
        assert_eq!(observer.is_speculative(), speculative);
    }
}

#[test]
fn foreground_and_speculative_startup_preserve_initial_state_and_irreversible_cancellation() {
    for speculative in [false, true] {
        let control = StreamControl::new(speculative);
        assert_eq!(control.is_speculative(), speculative);
        assert!(!control.decoder_running());
        assert!(!control.fully_decoded());
        assert_eq!(control.synthetic_silence_duration(), Duration::ZERO);
        assert!(control.take_io_error().is_none());
        assert!(control.check().is_ok());
        for chunks in [0, STARTUP_BUFFER_CHUNKS - 1] {
            assert!(control.wait_for_decode(chunks));
        }
        if !speculative {
            assert!(control.wait_for_decode(STARTUP_BUFFER_CHUNKS));
            assert!(control.wait_for_decode(usize::MAX));
        }
        control.cancel();
        control.cancel();
        control.promote();
        let error = control.check().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(error.to_string(), "audio preparation cancelled");
        for chunks in [0, STARTUP_BUFFER_CHUNKS - 1, STARTUP_BUFFER_CHUNKS] {
            assert!(!control.wait_for_decode(chunks));
        }
        assert!(!control.fully_decoded());
    }
}

#[test]
fn sleeping_decode_is_released_by_promotion_or_cancellation() {
    for promote in [false, true] {
        let control = StreamControl::new(true);
        let waiter = control.clone();
        let (completed_early, allowed) = observe_wait(
            move || waiter.wait_for_decode(STARTUP_BUFFER_CHUNKS),
            Duration::from_millis(50),
            || {
                if promote {
                    control.promote();
                } else {
                    control.cancel();
                }
            },
        );
        assert!(
            !completed_early,
            "speculative decoder must sleep at the target"
        );
        assert_eq!(allowed, promote);
    }
}

#[test]
fn cancelled_speculative_decoder_must_exit_before_preparation_can_be_reused() {
    let control = StreamControl::new(true);
    let guard = control.decoder_guard();
    control.cancel();
    let waiter = control.clone();
    let (completed_early, ()) = observe_wait(
        move || waiter.wait_until_reusable(),
        Duration::from_millis(50),
        || drop(guard),
    );
    assert!(
        !completed_early,
        "cancellation does not end a blocking decoder read"
    );
    assert!(!control.decoder_running());
    assert!(control.check().is_err());
}

#[test]
fn promotion_releases_preparation_without_waiting_for_the_playback_owned_decoder() {
    let control = StreamControl::new(true);
    let guard = control.decoder_guard();
    let waiter = control.clone();
    let (completed_early, ()) = observe_wait(
        move || waiter.wait_until_reusable(),
        Duration::from_millis(50),
        || control.promote(),
    );
    assert!(!completed_early);
    assert!(control.decoder_running());
    assert!(control.check().is_ok());
    drop(guard);
    assert!(!control.decoder_running());
}

#[test]
fn foreground_and_finished_speculative_decoders_do_not_block_preparation_reuse() {
    for speculative in [false, true] {
        let control = StreamControl::new(speculative);
        let guard = control.decoder_guard();
        let guard = if speculative {
            drop(guard);
            None
        } else {
            Some(guard)
        };
        let waiter = control.clone();
        let (completed_early, ()) = observe_wait(
            move || waiter.wait_until_reusable(),
            WAIT_TIMEOUT,
            || {
                control.promote();
                drop(guard);
            },
        );
        assert!(completed_early);
    }
}

#[test]
fn decoder_guard_releases_running_state_on_normal_exit_and_unwinding_without_marking_complete() {
    let control = StreamControl::new(true);
    let guard = control.decoder_guard();
    assert!(control.decoder_running());
    drop(guard);
    assert!(!control.decoder_running());
    let guard = control.decoder_guard();
    assert!(catch_unwind(AssertUnwindSafe(move || {
        let _guard = guard;
        panic!("injected decoder failure");
    }))
    .is_err());
    assert!(!control.decoder_running());
    assert!(control.check().is_ok());
    assert!(!control.fully_decoded());
}

#[test]
fn first_io_failure_is_retained_until_taken_and_later_failures_can_then_be_recorded() {
    let control = StreamControl::new(false);
    control.record_io_error(FailureError::new(StreamFailure::Timeout, "first failure"));
    control.record_io_error(FailureError::new(
        StreamFailure::Decode,
        "secondary failure",
    ));
    let error = control.take_io_error().unwrap();
    assert_eq!(error.kind, StreamFailure::Timeout);
    assert_eq!(error.detail, "first failure");
    assert!(control.take_io_error().is_none());
    control.record_io_error(FailureError::new(StreamFailure::Truncated, "later failure"));
    let error = control.take_io_error().unwrap();
    assert_eq!(error.kind, StreamFailure::Truncated);
    assert_eq!(error.detail, "later failure");
    assert!(control.take_io_error().is_none());
}

#[test]
fn poisoned_io_failure_slot_does_not_interrupt_stream_control() {
    let control = StreamControl::new(false);
    assert!(catch_unwind(AssertUnwindSafe(|| {
        let _guard = control.last_io_error.lock().unwrap();
        panic!("injected diagnostic-slot failure");
    }))
    .is_err());
    control.record_io_error(FailureError::new(StreamFailure::Timeout, "lost diagnostic"));
    assert!(control.take_io_error().is_none());
    assert!(control.check().is_ok());
    control.mark_data();
    control.promote();
    assert!(control.wait_for_decode(STARTUP_BUFFER_CHUNKS));
    control.cancel();
    assert!(control.check().is_err());
}

#[test]
fn data_arrival_and_promotion_refresh_the_monotonic_clock_and_future_readings_saturate() {
    let control = StreamControl::new(true);
    for promote in [false, true] {
        control.last_data_ms.store(u64::MAX, Ordering::Relaxed);
        assert_eq!(control.idle(), Duration::ZERO);
        let before = now_ms();
        if promote {
            control.promote();
        } else {
            control.mark_data();
        }
        let recorded = control.last_data_ms.load(Ordering::Relaxed);
        let after = now_ms();
        assert!((before..=after).contains(&recorded));
        assert!(control.idle() <= Duration::from_millis(now_ms().saturating_sub(before)));
    }
}

#[test]
fn synthetic_silence_counts_frames_using_the_last_reported_rate_and_handles_missing_rate() {
    for (frames, rate) in [(0, 48_000), (1, 44_100), (2_500, 1_000)] {
        let control = StreamControl::new(false);
        for _ in 0..frames {
            control.mark_synthetic_silence_frame(SampleRate::new(rate).unwrap());
        }
        assert_eq!(
            control.synthetic_silence_duration(),
            Duration::from_secs_f64(frames as f64 / f64::from(rate))
        );
    }
    let control = StreamControl::new(false);
    control.synthetic_silence_frames.store(1, Ordering::Relaxed);
    assert_eq!(control.synthetic_silence_duration(), Duration::ZERO);
    control.mark_synthetic_silence_frame(SampleRate::new(1_000).unwrap());
    control.mark_synthetic_silence_frame(SampleRate::new(2_000).unwrap());
    assert_eq!(
        control.synthetic_silence_duration(),
        Duration::from_micros(1_500)
    );
}
