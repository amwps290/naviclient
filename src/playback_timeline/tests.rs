use super::*;

fn timeline(position: Duration, duration: Option<Duration>) -> PlaybackTimeline {
    PlaybackTimeline { position, duration }
}

fn song(duration: Option<i32>) -> Song {
    Song {
        duration,
        ..Default::default()
    }
}

#[test]
fn seconds_seeks_clamp_to_track_bounds() {
    let duration = Some(Duration::from_secs(180));
    assert_eq!(
        timeline(Duration::from_secs(3), duration).seek_seconds(-5),
        Duration::ZERO
    );
    assert_eq!(
        timeline(Duration::from_secs(178), duration).seek_seconds(5),
        Duration::from_secs(180)
    );
    assert_eq!(
        timeline(Duration::from_secs(60), duration).seek_seconds(5),
        Duration::from_secs(65)
    );
    assert_eq!(
        timeline(Duration::from_secs(60), duration).seek_seconds(-5),
        Duration::from_secs(55)
    );
}

#[test]
fn microsecond_seeking_handles_negative_and_extreme_offsets() {
    let timeline = timeline(Duration::from_secs(10), Some(Duration::from_secs(20)));
    assert_eq!(timeline.seek_micros(-5_000_000), Duration::from_secs(5));
    assert_eq!(timeline.seek_micros(i64::MIN), Duration::ZERO);
    assert_eq!(timeline.seek_micros(i64::MAX), Duration::from_secs(20));
}

#[test]
fn unknown_duration_keeps_relative_forward_progress() {
    let timeline = timeline(Duration::from_secs(60), None);
    assert_eq!(timeline.seek_seconds(5), Duration::from_secs(65));
    assert_eq!(timeline.seek_seconds(-5), Duration::from_secs(55));
    assert_eq!(timeline.seek_seconds(0), Duration::from_secs(60));
}

#[test]
fn minimum_negative_seconds_reaches_the_start_without_overflow() {
    assert_eq!(
        timeline(Duration::from_secs(60), Some(Duration::from_secs(180))).seek_seconds(i64::MIN),
        Duration::ZERO
    );
}

#[test]
fn maximum_position_saturates_before_applying_bounds() {
    let unbounded = timeline(Duration::MAX, None);
    assert_eq!(unbounded.seek_seconds(5), Duration::MAX);
    assert_eq!(unbounded.seek_micros(5), Duration::MAX);
    let bounded = timeline(Duration::MAX, Some(Duration::from_secs(180)));
    assert_eq!(bounded.seek_seconds(i64::MAX), Duration::from_secs(180));
    assert_eq!(bounded.seek_micros(i64::MAX), Duration::from_secs(180));
}

#[test]
fn unbounded_seeks_support_the_full_signed_offset_range() {
    assert_eq!(
        timeline(Duration::from_secs(60), None).seek_seconds(i64::MAX),
        Duration::from_secs(i64::MAX as u64 + 60)
    );
    assert_eq!(
        timeline(Duration::MAX, None).seek_seconds(i64::MIN),
        Duration::new(i64::MAX as u64, 999_999_999)
    );
    assert_eq!(
        timeline(Duration::ZERO, None).seek_micros(i64::MAX),
        Duration::from_micros(i64::MAX as u64)
    );
}

#[test]
fn seeks_preserve_subsecond_position_and_microsecond_offsets() {
    let timeline = timeline(Duration::from_nanos(1_234_567_890), None);
    assert_eq!(
        timeline.seek_seconds(1),
        Duration::from_nanos(2_234_567_890)
    );
    assert_eq!(timeline.seek_micros(1), Duration::from_nanos(1_234_568_890));
    assert_eq!(
        timeline.seek_micros(-1),
        Duration::from_nanos(1_234_566_890)
    );
}

#[test]
fn all_relative_seeks_observe_a_known_end_even_from_an_invalid_position() {
    let timeline = timeline(Duration::from_secs(190), Some(Duration::from_secs(180)));
    assert_eq!(timeline.seek_seconds(-5), Duration::from_secs(180));
    assert_eq!(timeline.seek_micros(-5_000_000), Duration::from_secs(180));
    assert_eq!(timeline.seek_seconds(-20), Duration::from_secs(170));
    assert_eq!(timeline.seek_micros(0), Duration::from_secs(180));
    let zero_length = PlaybackTimeline {
        position: Duration::from_secs(10),
        duration: Some(Duration::ZERO),
    };
    assert_eq!(zero_length.seek_seconds(5), Duration::ZERO);
    assert_eq!(zero_length.seek_micros(-1), Duration::ZERO);
}

#[test]
fn seconds_and_microseconds_controls_agree_for_equivalent_offsets() {
    for duration in [None, Some(Duration::from_secs(120))] {
        let timeline = timeline(Duration::from_millis(60_123), duration);
        for seconds in [-120, -5, -1, 0, 1, 5, 120] {
            assert_eq!(
                timeline.seek_seconds(seconds),
                timeline.seek_micros(seconds * 1_000_000)
            );
        }
    }
}

#[test]
fn inactive_audio_uses_song_metadata_and_preserves_observed_position() {
    let song = song(Some(180));
    for paused in [false, true] {
        let playback = PlaybackState {
            paused,
            position: Duration::from_millis(45_125),
            duration: Some(Duration::from_secs(999)),
            ..Default::default()
        };
        assert_eq!(
            PlaybackTimeline::for_song(&playback, &song),
            timeline(
                Duration::from_millis(45_125),
                Some(Duration::from_secs(180))
            )
        );
    }
}

#[test]
fn active_audio_takes_precedence_even_when_paused_or_duration_is_zero() {
    let song = song(Some(180));
    for paused in [false, true] {
        for duration in [Duration::ZERO, Duration::from_millis(190_125)] {
            let playback = PlaybackState {
                active: true,
                paused,
                duration: Some(duration),
                ..Default::default()
            };
            assert_eq!(
                PlaybackTimeline::for_song(&playback, &song).duration,
                Some(duration)
            );
        }
    }
}

#[test]
fn unknown_audio_duration_falls_back_to_a_valid_metadata_hint() {
    for active in [false, true] {
        let playback = PlaybackState {
            active,
            ..Default::default()
        };
        for (declared, expected) in [
            (None, None),
            (Some(-1), None),
            (Some(i32::MIN), None),
            (Some(0), Some(Duration::ZERO)),
            (Some(180), Some(Duration::from_secs(180))),
            (Some(i32::MAX), Some(Duration::from_secs(i32::MAX as u64))),
        ] {
            assert_eq!(
                PlaybackTimeline::for_song(&playback, &song(declared)).duration,
                expected
            );
            assert_eq!(song_duration_hint(&song(declared)), expected);
        }
    }
}
