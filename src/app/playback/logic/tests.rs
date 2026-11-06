use super::*;

#[test]
fn preloading_waits_for_safe_current_audio_buffer_and_near_end() {
    let mut playback = crate::audio::PlaybackState {
        active: true,
        duration: Some(Duration::from_secs(180)),
        position: Duration::from_secs(165),
        fully_buffered: true,
        ..Default::default()
    };
    assert!(super::should_preload_next(&playback));
    playback.fully_buffered = false;
    assert!(!super::should_preload_next(&playback));
    playback.fully_buffered = true;
    playback.position = Duration::from_secs(164);
    assert!(!super::should_preload_next(&playback));
    playback.position = Duration::from_secs(165);
    playback.paused = true;
    assert!(!super::should_preload_next(&playback));
    playback.paused = false;
    playback.duration = None;
    assert!(!super::should_preload_next(&playback));
    playback.duration = Some(playback.position);
    assert!(!super::should_preload_next(&playback));
    playback.duration = Some(Duration::from_secs(180));
    playback.error = Some("stream failed".into());
    assert!(!super::should_preload_next(&playback));
}

#[test]
fn scrobbles_at_half_duration_or_four_minutes() {
    for (position, duration, expected) in [
        (10, Some(30), false),
        (15, Some(30), true),
        (241, Some(600), true),
        (240, None, true),
        (120, None, false),
        (0, Some(300), false),
    ] {
        assert_eq!(
            should_scrobble(&PlaybackState {
                active: true,
                position: Duration::from_secs(position),
                duration: duration.map(Duration::from_secs),
                ..Default::default()
            }),
            expected
        );
    }
}

#[test]
fn scrobble_waits_for_output_beyond_a_pending_start_or_seek_target() {
    use crate::audio::test_transport::Fixture;

    let mut fixture = Fixture::new(PlaybackState::default());
    fixture.audio.play(
        "test".into(),
        "song".into(),
        Some(Duration::from_secs(120)),
        1.0,
        Duration::from_secs(70),
        None,
    );
    assert!(!should_scrobble(&fixture.audio.progress()));
    let playback = PlaybackState {
        active: true,
        position: Duration::from_secs(70),
        duration: Some(Duration::from_secs(120)),
        ..Default::default()
    };
    fixture.publish_next(playback.clone());
    assert!(!should_scrobble(&fixture.audio.progress()));
    fixture.publish(PlaybackState {
        output_connected: true,
        ..playback
    });
    assert!(should_scrobble(&fixture.audio.progress()));
    fixture.audio.pause();
    assert!(!should_scrobble(&fixture.audio.progress()));
    fixture.audio.seek(Duration::from_secs(100));
    fixture.audio.resume();
    assert!(!should_scrobble(&fixture.audio.progress()));
    fixture.exit_with_error("output closed");
    assert!(!should_scrobble(&fixture.audio.progress()));
}
