use super::{DesktopState, TrackNotificationUpdate::*};
use crate::audio::PlaybackState;

fn playing(paused: bool) -> PlaybackState {
    PlaybackState {
        active: true,
        paused,
        ..Default::default()
    }
}

#[test]
fn notification_waits_for_audio_before_recording_the_track() {
    let mut state = DesktopState::default();
    for _ in 0..3 {
        assert_eq!(
            state.track_notification_update(true, false, Some("server"), Some("song")),
            CancelPending
        );
    }
    assert_eq!(
        state.track_notification_update(true, true, Some("server"), Some("song")),
        Send
    );
    assert_eq!(
        state.track_notification_update(true, true, Some("server"), Some("song")),
        Unchanged
    );
}

#[test]
fn repeated_playback_and_reconnection_of_audio_do_not_repeat_notifications() {
    let mut state = DesktopState::default();
    assert_eq!(
        state.track_notification_update(true, true, None, Some("song")),
        Send
    );
    for _ in 0..3 {
        assert_eq!(
            state.track_notification_update(true, false, None, Some("song")),
            CancelPending
        );
        assert_eq!(
            state.track_notification_update(true, true, None, Some("song")),
            Unchanged
        );
    }
}

#[test]
fn a_changed_track_can_notify_again_when_returning_to_an_earlier_song() {
    let mut state = DesktopState::default();
    for id in ["first", "second", "first"] {
        assert_eq!(
            state.track_notification_update(true, true, None, Some(id)),
            Send
        );
    }
}

#[test]
fn disabled_notifications_do_not_record_unannounced_tracks() {
    let mut state = DesktopState::default();
    assert_eq!(
        state.track_notification_update(false, true, None, Some("song")),
        CancelPending
    );
    assert_eq!(
        state.track_notification_update(true, true, None, Some("song")),
        Send
    );
    assert_eq!(
        state.track_notification_update(false, true, None, Some("other")),
        CancelPending
    );
    assert_eq!(
        state.track_notification_update(true, true, None, Some("other")),
        Send
    );
}

#[test]
fn toggling_notifications_preserves_deduplication_for_an_announced_track() {
    let mut state = DesktopState::default();
    assert_eq!(
        state.track_notification_update(true, true, None, Some("song")),
        Send
    );
    assert_eq!(
        state.track_notification_update(false, true, None, Some("song")),
        CancelPending
    );
    assert_eq!(
        state.track_notification_update(true, true, None, Some("song")),
        Unchanged
    );
}

#[test]
fn empty_playback_clears_track_memory_even_while_notifications_are_disabled() {
    for enabled in [false, true] {
        let mut state = DesktopState::default();
        assert_eq!(
            state.track_notification_update(true, true, None, Some("song")),
            Send
        );
        assert_eq!(
            state.track_notification_update(enabled, false, None, None),
            CancelPending
        );
        assert_eq!(
            state.track_notification_update(true, true, None, Some("song")),
            Send
        );
    }
}

#[test]
fn same_song_ids_on_distinct_servers_have_distinct_notification_identities() {
    let mut state = DesktopState::default();
    for server in [None, Some("first"), Some("second"), None] {
        assert_eq!(
            state.track_notification_update(true, true, server, Some("song")),
            Send
        );
        assert_eq!(
            state.track_notification_update(true, true, server, Some("song")),
            Unchanged
        );
    }
}

#[test]
fn server_reconnect_resets_notifications_and_preserves_tray_deduplication() {
    let mut state = DesktopState::default();
    assert_eq!(state.tray_paused_update(&playing(true)), Some(true));
    assert_eq!(
        state.track_notification_update(true, true, Some("server"), Some("song")),
        Send
    );
    state.reset_track();
    state.reset_track();
    assert_eq!(state.tray_paused_update(&playing(true)), None);
    assert_eq!(
        state.track_notification_update(true, true, Some("server"), Some("song")),
        Send
    );
}

#[test]
fn tray_sync_reports_initial_state_and_each_actual_pause_change() {
    for initial in [false, true] {
        let mut state = DesktopState::default();
        assert_eq!(state.tray_paused_update(&playing(initial)), Some(initial));
        assert_eq!(state.tray_paused_update(&playing(initial)), None);
        assert_eq!(state.tray_paused_update(&playing(!initial)), Some(!initial));
        assert_eq!(state.tray_paused_update(&playing(!initial)), None);
        assert_eq!(state.tray_paused_update(&playing(initial)), Some(initial));
    }
}

#[test]
fn tray_shows_play_when_idle_stopped_or_worker_closed_and_pause_while_preparing() {
    use crate::audio::test_transport::Fixture;

    let mut fixture = Fixture::new(PlaybackState::default());
    let mut state = DesktopState::default();
    assert_eq!(state.tray_paused_update(&fixture.audio.state()), Some(true));
    fixture
        .audio
        .play("test".into(), "song".into(), None, 1.0, Default::default());
    assert_eq!(
        state.tray_paused_update(&fixture.audio.state()),
        Some(false)
    );
    fixture.audio.stop();
    assert_eq!(state.tray_paused_update(&fixture.audio.state()), Some(true));
    fixture
        .audio
        .play("test".into(), "song".into(), None, 1.0, Default::default());
    assert_eq!(
        state.tray_paused_update(&fixture.audio.state()),
        Some(false)
    );
    fixture.exit_with_error("output failed");
    assert_eq!(state.tray_paused_update(&fixture.audio.state()), Some(true));
}

#[test]
fn accepted_start_and_buffering_do_not_record_a_track_notification() {
    use crate::audio::test_transport::Fixture;

    let fixture = Fixture::new(PlaybackState {
        output_connected: true,
        ..playing(false)
    });
    let mut state = DesktopState::default();
    fixture
        .audio
        .play("test".into(), "next".into(), None, 1.0, Default::default());
    assert_eq!(
        state.track_notification_update(true, fixture.audio.progress().active, None, Some("next")),
        CancelPending
    );
    fixture.publish_next(playing(false));
    assert_eq!(
        state.track_notification_update(true, fixture.audio.progress().active, None, Some("next")),
        CancelPending
    );
    fixture.publish(PlaybackState {
        output_connected: true,
        ..playing(true)
    });
    assert_eq!(
        state.track_notification_update(true, fixture.audio.progress().active, None, Some("next")),
        Send
    );
    assert_eq!(
        state.track_notification_update(true, fixture.audio.progress().active, None, Some("next")),
        Unchanged
    );
}
