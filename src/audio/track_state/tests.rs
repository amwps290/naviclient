use super::*;

#[test]
fn seek_keeps_resource_headers_proxy_and_temporary_cache_policy() {
    use crate::source::{CachePolicy, HttpResource};
    use reqwest::header::{HeaderMap, HeaderValue, REFERER};
    let mut headers = HeaderMap::new();
    headers.insert(
        REFERER,
        HeaderValue::from_static("https://www.bilibili.com/"),
    );
    let mut resource = HttpResource::new("https://cdn.test/audio?signature=secret")
        .with_headers(headers)
        .with_route(crate::models::network::NetworkRoute::Proxy(
            "http://proxy.test:7890".into(),
        ));
    resource = resource.with_cache_policy(CachePolicy::Temporary { max_bytes: 10 });
    resource = resource.with_mime_hint(Some("audio/mp4".into()));
    let mut track = TrackState::default();
    track.begin_play(
        resource,
        "stable-key".into(),
        Some(Duration::from_secs(180)),
        1.0,
        Duration::ZERO,
    );
    let (position, seek) = track
        .prepare_seek(Duration::from_secs(120), PathBuf::from("unused"))
        .unwrap();
    assert_eq!(position, Duration::from_secs(120));
    assert_eq!(seek.cache_key, "stable-key");
    assert_eq!(seek.url.headers()[REFERER], "https://www.bilibili.com/");
    assert_eq!(
        seek.url.route(),
        &crate::models::network::NetworkRoute::Proxy("http://proxy.test:7890".into())
    );
    assert_eq!(
        seek.url.cache_policy(),
        CachePolicy::Temporary { max_bytes: 10 }
    );
    assert_eq!(seek.url.mime_hint(), Some("audio/mp4"));
}
use crate::audio::error::StreamFailure;
use crate::audio::preparation_state::PreparationKind;
use crate::audio::stream_cache::BufferProgress;
use crate::audio::stream_control::StreamControl;
use crate::audio::track_transition::Transition;
use rodio::SampleRate;
use std::sync::{Arc, Mutex};

fn info(duration: Option<Duration>) -> TrackInfo {
    let control = StreamControl::new(false);
    control.mark_fully_decoded();
    TrackInfo {
        duration,
        error: Arc::new(Mutex::new(None)),
        buffer: Arc::new(BufferProgress::new(25, Some(100))),
        control,
        loudness: Arc::new(Mutex::new(Some((-12.0, 0.25)))),
    }
}

fn ready() -> (TrackState, TrackInfo) {
    let info = info(Some(Duration::from_secs(120)));
    let mut track = TrackState::default();
    track.begin_play(
        "https://one.test/a".into(),
        "a".into(),
        info.duration,
        0.5,
        Duration::ZERO,
    );
    track.accept_ready(info.clone());
    (track, info)
}

fn output(position: Duration, empty: bool) -> OutputSnapshot {
    OutputSnapshot {
        position,
        empty,
        paused: false,
    }
}

fn snapshot(track: &TrackState, position: Duration, empty: bool) -> PlaybackState {
    track
        .snapshot(
            output(position, empty),
            &PreparationState::default(),
            Instant::now(),
        )
        .playback
}

fn transition(id: u64, boundary: Duration) -> TrackTransition {
    TrackTransition {
        id,
        boundary,
        loudness_db: -12.0,
        loudness_peak: 0.25,
    }
}

#[test]
fn idle_and_stopped_outputs_have_no_song_clock_or_natural_end() {
    let idle = TrackState::default();
    let state = snapshot(&idle, Duration::from_secs(999), true);
    assert_eq!(state.position, Duration::ZERO);
    assert_eq!(state.duration, None);
    assert!(!state.active && !state.ended);
    assert!(!state.output_connected);
    assert_eq!(state.error, None);
    let (mut track, _) = ready();
    track.stop();
    assert_eq!(snapshot(&track, Duration::from_secs(999), true), state);
    assert!(track
        .prepare_seek(Duration::from_secs(20), PathBuf::from("/unused"))
        .is_none());
}

#[test]
fn new_play_detaches_previous_diagnostics_loudness_and_errors() {
    let (mut track, old) = ready();
    track.record_error(FailureError::new(
        StreamFailure::Output,
        "old output failed",
    ));
    track.begin_play(
        "https://two.test/b".into(),
        "b".into(),
        Some(Duration::from_secs(60)),
        1.0,
        Duration::from_secs(5),
    );
    *old.error.lock().unwrap() = Some(FailureError::new(StreamFailure::Timeout, "old timeout"));
    *old.loudness.lock().unwrap() = Some((-1.0, 0.9));
    old.buffer.set_cached_bytes_for_test(100);
    let now = Instant::now();
    let mut preparation = PreparationState::default();
    preparation.begin(PreparationKind::Play, Duration::from_secs(5), now);
    let state = track
        .snapshot(output(Duration::ZERO, true), &preparation, now)
        .playback;
    assert_eq!(state.position, Duration::from_secs(5));
    assert_eq!(state.duration, Some(Duration::from_secs(60)));
    assert!(state.active && !state.ended);
    assert!(!state.output_connected);
    assert_eq!(state.buffered, Duration::ZERO);
    assert!(!state.fully_buffered);
    assert_eq!(state.loudness_db, 0.0);
    assert_eq!(state.loudness_peak, 0.0);
    assert_eq!(state.failure, None);
    assert_eq!(state.error, None);
    assert_eq!(state.stall, None);
    assert_eq!(state.transition_id, None);
}

#[test]
fn ready_track_uses_decoded_duration_buffer_progress_and_current_loudness() {
    let (track, info) = ready();
    let state = snapshot(&track, Duration::from_secs(30), false);
    assert!(state.output_connected);
    assert_eq!(state.position, Duration::from_secs(30));
    assert_eq!(state.buffered, Duration::from_secs(30));
    assert!(state.fully_buffered);
    assert_eq!((state.loudness_db, state.loudness_peak), (-12.0, 0.25));
    info.buffer.set_cached_bytes_for_test(150);
    info.control.set_fully_decoded_for_test(false);
    let state = snapshot(&track, Duration::from_secs(130), false);
    assert_eq!(state.position, Duration::from_secs(120));
    assert_eq!(state.buffered, Duration::from_secs(120));
    assert!(!state.fully_buffered);
}

#[test]
fn unknown_decoded_duration_keeps_the_clock_unbounded_and_buffer_time_unknown() {
    let (mut track, _) = ready();
    track.accept_ready(info(None));
    let state = snapshot(&track, Duration::from_secs(150), false);
    assert_eq!(state.duration, None);
    assert_eq!(state.position, Duration::from_secs(150));
    assert_eq!(state.buffered, Duration::ZERO);
}

#[test]
fn synthetic_silence_is_removed_from_output_clock() {
    let (track, info) = ready();
    for _ in 0..48 {
        info.control
            .mark_synthetic_silence_frame(SampleRate::new(48_000).unwrap());
    }
    assert_eq!(
        track.position(Duration::from_millis(10)),
        Duration::from_millis(9)
    );
    assert_eq!(track.position(Duration::from_micros(500)), Duration::ZERO);
}

#[test]
fn gapless_handoff_uses_next_source_clock_and_diagnostics_without_old_errors() {
    let (mut track, _) = ready();
    track.record_error(FailureError::new(
        StreamFailure::Output,
        "old output failure",
    ));
    let next_info = info(Some(Duration::from_secs(60)));
    *next_info.loudness.lock().unwrap() = Some((-6.0, 0.5));
    track.accept_gapless(
        AutoTrack {
            transition: Transition::new(7, next_info.duration),
            url: "https://two.test/b".into(),
            cache_key: "b".into(),
            gain: 0.8,
            info: next_info,
        },
        transition(7, Duration::from_secs(120)),
    );
    let state = snapshot(&track, Duration::from_millis(120_125), false);
    assert!(state.output_connected);
    assert_eq!(state.position, Duration::from_millis(125));
    assert_eq!(state.duration, Some(Duration::from_secs(60)));
    assert_eq!(state.transition_id, Some(7));
    assert_eq!((state.loudness_db, state.loudness_peak), (-6.0, 0.5));
    assert_eq!(track.gain(), 0.8);
    assert_eq!(snapshot(&track, Duration::from_secs(180), true).error, None);
    let (_, request) = track
        .prepare_seek(Duration::ZERO, PathBuf::from("/new-cache"))
        .unwrap();
    assert_eq!(request.url.url(), "https://two.test/b");
    assert_eq!(request.cache_key, "b");
    assert_eq!(request.cache_dir, PathBuf::from("/new-cache"));
}

#[test]
fn seek_after_gapless_keeps_occurrence_and_gain_but_replaces_decode_progress() {
    let (mut track, info) = ready();
    track.accept_gapless(
        AutoTrack {
            transition: Transition::new(7, info.duration),
            url: "https://one.test/a".into(),
            cache_key: "a".into(),
            gain: 0.5,
            info,
        },
        transition(7, Duration::from_secs(120)),
    );
    let (position, request) = track
        .prepare_seek(Duration::from_secs(150), PathBuf::from("/cache"))
        .unwrap();
    assert_eq!(position, Duration::from_secs(120));
    assert_eq!(request.duration, Some(Duration::from_secs(120)));
    let state = snapshot(&track, Duration::ZERO, false);
    assert!(!state.output_connected);
    assert_eq!(state.position, Duration::from_secs(120));
    assert_eq!(state.transition_id, Some(7));
    assert_eq!(state.buffered, Duration::ZERO);
    assert!(!state.fully_buffered);
    assert_eq!((state.loudness_db, state.loudness_peak), (-12.0, 0.25));
    assert_eq!(track.gain(), 0.5);
    let fresh = info_without_loudness();
    track.accept_ready(fresh);
    assert_eq!(snapshot(&track, Duration::ZERO, false).loudness_db, 0.0);
}

fn info_without_loudness() -> TrackInfo {
    let info = info(Some(Duration::from_secs(120)));
    *info.loudness.lock().unwrap() = None;
    info
}

#[test]
fn snapshot_preserves_device_information_and_seek_completion_serial() {
    let (track, _) = ready();
    let mut state = PlaybackState {
        seek_serial: 7,
        output_name: Some("DAC".into()),
        output_sample_rate: 96_000,
        output_channels: 2,
        ..Default::default()
    };
    track
        .snapshot(
            output(Duration::from_secs(30), false),
            &PreparationState::default(),
            Instant::now(),
        )
        .apply(&mut state);
    assert_eq!(state.seek_serial, 7);
    assert_eq!(state.output_name.as_deref(), Some("DAC"));
    assert_eq!(state.output_sample_rate, 96_000);
    assert_eq!(state.output_channels, 2);
    assert_eq!(state.position, Duration::from_secs(30));
    let mut track = track;
    track.stop();
    track
        .snapshot(
            output(Duration::ZERO, true),
            &PreparationState::default(),
            Instant::now(),
        )
        .apply(&mut state);
    assert_eq!(state.seek_serial, 7);
    assert_eq!(state.output_name.as_deref(), Some("DAC"));
}

#[test]
fn preparing_uses_explicit_pause_intent_and_never_reports_natural_end() {
    let now = Instant::now();
    let mut track = TrackState::default();
    track.begin_play(
        "https://one.test/a".into(),
        "a".into(),
        None,
        1.0,
        Duration::from_secs(30),
    );
    let mut preparation = PreparationState::default();
    preparation.begin(PreparationKind::Play, Duration::from_secs(30), now);
    let state = track
        .snapshot(
            OutputSnapshot {
                position: Duration::ZERO,
                empty: true,
                paused: true,
            },
            &preparation,
            now,
        )
        .playback;
    assert!(state.active && !state.ended && !state.paused);
    assert!(!state.output_connected);
    preparation.set_paused(true);
    let state = track
        .snapshot(
            output(Duration::ZERO, true),
            &preparation,
            now + Duration::from_millis(4500),
        )
        .playback;
    assert!(state.active && !state.ended && state.paused);
    assert!(!state.output_connected);
    assert_eq!(state.stall, Some(Duration::from_secs(4)));
}

#[test]
fn terminal_errors_wait_for_drained_output_and_keep_failure_classification() {
    let (track, info) = ready();
    *info.error.lock().unwrap() = Some(FailureError::new(
        StreamFailure::Timeout,
        "stream timed out",
    ));
    assert_eq!(snapshot(&track, Duration::from_secs(30), false).error, None);
    let state = snapshot(&track, Duration::from_secs(30), true);
    assert!(!state.active && !state.ended);
    assert!(!state.output_connected);
    assert_eq!(state.failure, Some(StreamFailure::Timeout));
    assert!(state.error.unwrap().contains("stream timed out"));
}

#[test]
fn output_error_does_not_discard_current_buffering_or_live_stream() {
    let (mut track, _) = ready();
    track.record_error(FailureError::new(
        StreamFailure::Output,
        "device unavailable",
    ));
    let state = snapshot(&track, Duration::from_secs(30), false);
    assert!(state.active && state.fully_buffered);
    assert_eq!(state.buffered, Duration::from_secs(30));
    assert_eq!(state.error, None);
    let state = snapshot(&track, Duration::from_secs(120), true);
    assert_eq!(state.failure, Some(StreamFailure::Output));
    assert!(!state.ended);
}

#[test]
fn terminal_log_is_once_per_occurrence_and_reset_by_play_and_seek() {
    let mut track = TrackState::default();
    assert!(!track.take_terminal_log(true, false));
    track.begin_play(
        "https://one.test/a".into(),
        "a".into(),
        None,
        1.0,
        Duration::ZERO,
    );
    assert!(!track.take_terminal_log(true, true));
    assert!(!track.take_terminal_log(false, false));
    assert!(track.take_terminal_log(true, false));
    assert!(!track.take_terminal_log(true, false));
    track
        .prepare_seek(Duration::from_secs(20), PathBuf::from("/unused"))
        .unwrap();
    assert!(track.take_terminal_log(true, false));
    track.stop();
    assert!(!track.take_terminal_log(true, false));
}

#[test]
fn stalls_are_visible_after_three_seconds_and_only_for_waiting_or_active_output() {
    for (elapsed, expected) in [
        (Duration::from_millis(2999), None),
        (Duration::from_secs(3), Some(Duration::from_secs(3))),
        (Duration::from_millis(4500), Some(Duration::from_secs(4))),
    ] {
        assert_eq!(stall_duration(Some(elapsed), true, true, None), expected);
        assert_eq!(stall_duration(None, false, false, Some(elapsed)), expected);
        assert_eq!(stall_duration(None, false, true, Some(elapsed)), None);
        assert_eq!(stall_duration(None, true, false, Some(elapsed)), None);
    }
    assert_eq!(stall_duration(None, false, false, None), None);
}
