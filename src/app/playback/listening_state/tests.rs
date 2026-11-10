use super::*;

fn test_id() -> PlaybackId {
    static ID: std::sync::OnceLock<PlaybackId> = std::sync::OnceLock::new();
    *ID.get_or_init(PlaybackId::new)
}

fn song(id: &str) -> Song {
    Song {
        id: id.into(),
        title: id.into(),
        ..Default::default()
    }
}

fn playing(position_ms: u64) -> PlaybackState {
    PlaybackState {
        active: true,
        position: Duration::from_millis(position_ms),
        ..Default::default()
    }
}

#[test]
fn playback_history_projection_preserves_order_duplicates_and_bound() {
    let mut persisted = ListeningHistory::default();
    for index in 0..205 {
        persisted.record_started(
            Song {
                id: format!("song-{}", index % 3),
                title: index.to_string(),
                ..Song::default()
            },
            index,
        );
    }
    let mut state = ListeningState::default();
    state.restore(None, persisted);
    assert_eq!(state.recent_songs().len(), 200);
    assert_eq!(state.recent_songs()[0].title, "204");
    assert_eq!(state.recent_songs()[1].title, "203");
    assert_eq!(state.recent_songs()[0].id, state.recent_songs()[3].id);

    assert_eq!(
        state.observe(
            Some(&song("new")),
            Some(test_id()),
            &playing(0),
            Instant::now(),
            || 205
        ),
        ListeningUpdate::Started
    );
    assert_eq!(state.recent_songs().len(), 200);
    assert_eq!(state.recent_songs()[0].id, "new");
    assert_eq!(state.history().events.len(), 206);
    assert_eq!(state.history().events.last().unwrap().song.id, "new");
}

#[test]
fn listening_progress_rejects_seeks_and_counts_ordinary_output() {
    assert_eq!(
        listening_position_delta(Duration::from_secs(10), Duration::from_millis(10_250)),
        Duration::from_millis(250)
    );
    assert_eq!(
        listening_position_delta(Duration::from_secs(10), Duration::from_secs(9)),
        Duration::ZERO
    );
    assert_eq!(
        listening_position_delta(Duration::from_secs(10), Duration::from_secs(15)),
        Duration::ZERO
    );
}

#[test]
fn restored_paused_audio_does_not_start_a_listening_event() {
    let mut state = ListeningState::default();
    let song = song("song");
    let now = Instant::now();
    assert_eq!(
        state.observe(
            Some(&song),
            Some(test_id()),
            &PlaybackState::default(),
            now,
            || panic!("no start")
        ),
        ListeningUpdate::Unchanged
    );
    assert!(state.history().events.is_empty());
    assert!(state.recent_songs().is_empty());

    assert_eq!(
        state.observe(Some(&song), Some(test_id()), &playing(40_000), now, || 10),
        ListeningUpdate::Started
    );
    assert_eq!(state.history().events[0].started_at, 10);
    assert_eq!(state.history().events[0].listened_seconds, 0);
    assert_eq!(state.recent_songs()[0].id, song.id);
}

#[test]
fn fractional_progress_accumulates_before_the_event_is_finished() {
    let mut state = ListeningState::default();
    let song = song("song");
    let now = Instant::now();
    state.observe(Some(&song), Some(test_id()), &playing(10_000), now, || 10);
    for (elapsed, position) in [(250, 10_250), (500, 10_500), (1000, 11_000)] {
        assert_eq!(
            state.observe(
                Some(&song),
                Some(test_id()),
                &playing(position),
                now + Duration::from_millis(elapsed),
                || panic!("already started")
            ),
            ListeningUpdate::Unchanged
        );
    }
    assert!(state.finish(true));
    assert_eq!(state.history().events[0].listened_seconds, 1);
    assert!(state.history().events[0].completed);
    assert!(!state.finish(false));
    assert!(state.history().events[0].completed);
}

#[test]
fn paused_positions_and_seeks_are_excluded_from_listened_time() {
    let mut state = ListeningState::default();
    let song = song("song");
    let now = Instant::now();
    state.observe(Some(&song), Some(test_id()), &playing(0), now, || 10);
    state.observe(
        Some(&song),
        Some(test_id()),
        &playing(1000),
        now + Duration::from_secs(1),
        || 10,
    );
    let paused = PlaybackState {
        paused: true,
        ..playing(2000)
    };
    state.observe(
        Some(&song),
        Some(test_id()),
        &paused,
        now + Duration::from_secs(2),
        || 10,
    );
    let paused = PlaybackState {
        position: Duration::from_secs(4),
        ..paused
    };
    state.observe(
        Some(&song),
        Some(test_id()),
        &paused,
        now + Duration::from_secs(3),
        || 10,
    );

    for (elapsed, position) in [(4, 4500), (5, 10_000), (6, 11_000), (7, 5000), (8, 6000)] {
        state.observe(
            Some(&song),
            Some(test_id()),
            &playing(position),
            now + Duration::from_secs(elapsed),
            || 10,
        );
    }
    assert!(state.finish(false));
    assert_eq!(state.history().events[0].listened_seconds, 3);
    assert!(!state.history().events[0].completed);
    assert_eq!(state.history().events.len(), 1);
}

#[test]
fn reconnecting_the_same_song_keeps_its_start_and_accumulated_time() {
    let mut state = ListeningState::default();
    let song = song("song");
    let now = Instant::now();
    state.observe(Some(&song), Some(test_id()), &playing(20_000), now, || 10);
    state.observe(
        Some(&song),
        Some(test_id()),
        &playing(21_000),
        now + Duration::from_secs(1),
        || 10,
    );
    let interrupted = PlaybackState {
        active: false,
        error: Some("connection interrupted".into()),
        ..playing(20_500)
    };
    assert_eq!(
        state.observe(
            Some(&song),
            Some(test_id()),
            &interrupted,
            now + Duration::from_secs(2),
            || panic!("same occurrence")
        ),
        ListeningUpdate::Unchanged
    );
    state.observe(
        Some(&song),
        Some(test_id()),
        &playing(21_000),
        now + Duration::from_secs(3),
        || panic!("same occurrence"),
    );
    assert!(state.finish(false));
    assert_eq!(state.history().events.len(), 1);
    assert_eq!(state.recent_songs().len(), 1);
    assert_eq!(state.history().events[0].started_at, 10);
    assert_eq!(state.history().events[0].listened_seconds, 1);
}

#[test]
fn replaying_the_same_song_creates_a_separate_occurrence() {
    let mut state = ListeningState::default();
    let song = song("song");
    let now = Instant::now();
    state.observe(Some(&song), Some(test_id()), &playing(0), now, || 10);
    state.observe(
        Some(&song),
        Some(test_id()),
        &playing(1000),
        now + Duration::from_secs(1),
        || 10,
    );
    assert!(state.finish(false));
    assert_eq!(
        state.observe(
            Some(&song),
            Some(test_id()),
            &playing(0),
            now + Duration::from_secs(2),
            || 20
        ),
        ListeningUpdate::Started
    );
    state.observe(
        Some(&song),
        Some(test_id()),
        &playing(2000),
        now + Duration::from_secs(4),
        || 20,
    );
    assert!(state.finish(true));

    assert_eq!(state.history().events.len(), 2);
    assert_eq!(state.recent_songs().len(), 2);
    assert_eq!(state.recent_songs()[0].id, state.recent_songs()[1].id);
    let first = &state.history().events[0];
    assert_eq!(
        (first.started_at, first.listened_seconds, first.completed),
        (10, 1, false)
    );
    let second = &state.history().events[1];
    assert_eq!(
        (second.started_at, second.listened_seconds, second.completed),
        (20, 2, true)
    );
}

#[test]
fn switching_songs_updates_the_old_event_before_recording_the_new_one() {
    let mut state = ListeningState::default();
    let first = song("first");
    let second = song("second");
    let now = Instant::now();
    state.observe(Some(&first), Some(test_id()), &playing(0), now, || 10);
    state.observe(
        Some(&first),
        Some(test_id()),
        &playing(1000),
        now + Duration::from_secs(1),
        || 10,
    );
    assert_eq!(
        state.observe(
            Some(&second),
            Some(test_id()),
            &playing(0),
            now + Duration::from_secs(2),
            || 20
        ),
        ListeningUpdate::Started
    );
    assert_eq!(state.history().events[0].listened_seconds, 1);
    assert!(!state.history().events[0].completed);
    assert_eq!(state.history().events[1].started_at, 20);
    assert_eq!(state.history().events[1].listened_seconds, 0);
    assert_eq!(
        state
            .recent_songs()
            .iter()
            .map(|song| song.id.as_str())
            .collect::<Vec<_>>(),
        ["second", "first"]
    );
}

#[test]
fn selecting_inactive_audio_finishes_the_previous_event_without_starting_another() {
    let mut state = ListeningState::default();
    let first = song("first");
    let second = song("second");
    let now = Instant::now();
    state.observe(Some(&first), Some(test_id()), &playing(0), now, || 10);
    state.observe(
        Some(&first),
        Some(test_id()),
        &playing(1000),
        now + Duration::from_secs(1),
        || 10,
    );
    let now = now + Duration::from_secs(2);
    assert_eq!(
        state.observe(
            Some(&second),
            Some(test_id()),
            &PlaybackState::default(),
            now,
            || panic!("not active")
        ),
        ListeningUpdate::Updated
    );
    assert_eq!(state.history().events[0].listened_seconds, 1);
    assert_eq!(state.history().events.len(), 1);
    assert_eq!(state.recent_songs()[0].id, "first");
    assert_eq!(
        state.observe(
            Some(&second),
            Some(test_id()),
            &PlaybackState::default(),
            now,
            || panic!("not active")
        ),
        ListeningUpdate::Unchanged
    );
    assert_eq!(
        state.observe(Some(&second), Some(test_id()), &playing(0), now, || 20),
        ListeningUpdate::Started
    );
    assert_eq!(state.history().events.len(), 2);
}

#[test]
fn stopping_finishes_the_partial_event_once() {
    let mut state = ListeningState::default();
    let song = song("song");
    let now = Instant::now();
    state.observe(Some(&song), Some(test_id()), &playing(0), now, || 10);
    state.observe(
        Some(&song),
        Some(test_id()),
        &playing(1000),
        now + Duration::from_secs(1),
        || 10,
    );
    assert_eq!(
        state.observe(
            None,
            Some(test_id()),
            &PlaybackState::default(),
            now + Duration::from_secs(2),
            || panic!("no song")
        ),
        ListeningUpdate::Updated
    );
    assert_eq!(state.history().events[0].listened_seconds, 1);
    assert!(!state.history().events[0].completed);
    assert_eq!(
        state.observe(
            None,
            Some(test_id()),
            &PlaybackState::default(),
            now + Duration::from_secs(3),
            || panic!("no song")
        ),
        ListeningUpdate::Unchanged
    );
    assert!(!state.finish(true));
    assert!(!state.history().events[0].completed);
}

#[test]
fn progress_saves_are_throttled_to_fifteen_seconds() {
    let mut state = ListeningState::default();
    let song = song("song");
    let now = Instant::now();
    state.observe(Some(&song), Some(test_id()), &playing(0), now, || 10);
    for elapsed in 1..=30 {
        let update = state.observe(
            Some(&song),
            Some(test_id()),
            &playing(elapsed * 1000),
            now + Duration::from_secs(elapsed),
            || panic!("already started"),
        );
        if elapsed % 15 == 0 {
            assert_eq!(update, ListeningUpdate::Updated);
            assert_eq!(state.history().events[0].listened_seconds, elapsed as u32);
        } else {
            assert_eq!(update, ListeningUpdate::Unchanged);
        }
    }
    assert!(state.finish(true));
    assert_eq!(state.history().events[0].listened_seconds, 30);
    assert_eq!(state.history().events.len(), 1);
    assert_eq!(state.recent_songs().len(), 1);
}

#[test]
fn restoring_another_server_replaces_both_histories_and_discards_active_progress() {
    let mut state = ListeningState::default();
    let old = song("shared-id");
    let now = Instant::now();
    state.observe(Some(&old), Some(test_id()), &playing(0), now, || 10);
    state.observe(
        Some(&old),
        Some(test_id()),
        &playing(1000),
        now + Duration::from_secs(1),
        || 10,
    );

    let mut restored = ListeningHistory::default();
    restored.record_started(
        Song {
            title: "Other server".into(),
            ..song("shared-id")
        },
        20,
    );
    state.restore(Some("other-server"), restored);
    assert!(!state.finish(true));
    assert_eq!(state.history().events.len(), 1);
    assert_eq!(state.history().events[0].started_at, 20);
    assert_eq!(state.history().events[0].listened_seconds, 0);
    assert!(!state.history().events[0].completed);
    assert_eq!(state.recent_songs()[0].title, "Other server");
    assert_eq!(state.server_id(), Some("other-server"));
    state.restore(None, ListeningHistory::default());
    assert_eq!(state.server_id(), None);
    assert!(state.history().events.is_empty());
    assert!(state.recent_songs().is_empty());
}

#[test]
fn invalid_song_ids_never_create_an_active_event_or_a_drawer_entry() {
    let mut state = ListeningState::default();
    let now = Instant::now();
    for id in ["", "  "] {
        assert_eq!(
            state.observe(
                Some(&song(id)),
                Some(test_id()),
                &playing(0),
                now,
                || panic!("invalid song")
            ),
            ListeningUpdate::Unchanged
        );
    }
    assert!(state.history().events.is_empty());
    assert!(state.recent_songs().is_empty());
    assert!(!state.finish(true));
}

#[test]
fn pending_progress_keeps_its_original_server_after_configuration_switches() {
    let mut config = crate::models::Config::default();
    let first = config.add_server();
    let second = config.add_server();
    assert!(config.switch_to(&first));
    let mut state = ListeningState::default();
    state.restore(config.active_server_id(), ListeningHistory::default());
    let song = song("song");
    let now = Instant::now();
    state.observe(Some(&song), Some(test_id()), &playing(0), now, || 10);
    state.observe(
        Some(&song),
        Some(test_id()),
        &playing(1000),
        now + Duration::from_secs(1),
        || 10,
    );

    assert!(config.switch_to(&second));
    assert!(state.finish(false));
    assert_eq!(state.server_id(), Some(first.as_str()));
    assert_eq!(config.active_server_id(), Some(second.as_str()));
    assert_eq!(state.history().events[0].listened_seconds, 1);

    state.restore(config.active_server_id(), ListeningHistory::default());
    assert_eq!(state.server_id(), Some(second.as_str()));
    assert!(state.history().events.is_empty());
    assert!(state.recent_songs().is_empty());
}

#[test]
fn removing_the_active_server_finishes_its_progress_before_cache_cleanup() {
    let mut config = crate::models::Config::default();
    let first = config.add_server();
    let second = config.add_server();
    assert!(config.switch_to(&first));
    let mut state = ListeningState::default();
    state.restore(config.active_server_id(), ListeningHistory::default());
    let song = song("song");
    let now = Instant::now();
    state.observe(Some(&song), Some(test_id()), &playing(0), now, || 10);
    state.observe(
        Some(&song),
        Some(test_id()),
        &playing(1000),
        now + Duration::from_secs(1),
        || 10,
    );

    assert!(config.remove_server(&first));
    assert_eq!(config.active_server_id(), Some(second.as_str()));
    assert!(state.finish(false));
    assert_eq!(state.server_id(), Some(first.as_str()));
    assert_eq!(state.history().events[0].listened_seconds, 1);
    // Once the caller deletes the old cache, reconnect's finish is a no-op.
    assert!(!state.finish(false));
    state.restore(config.active_server_id(), ListeningHistory::default());
    assert!(!state.finish(false));
    assert!(state.history().events.is_empty());
}

#[test]
fn a_new_occurrence_of_the_same_song_finishes_old_progress_without_an_explicit_finish() {
    let mut state = ListeningState::default();
    let song = song("same");
    let first = PlaybackId::new();
    let second = PlaybackId::new();
    let now = Instant::now();
    state.observe(Some(&song), Some(first), &playing(0), now, || 10);
    state.observe(
        Some(&song),
        Some(first),
        &playing(1000),
        now + Duration::from_secs(1),
        || panic!("same occurrence"),
    );
    assert_eq!(
        state.observe(
            Some(&song),
            Some(second),
            &playing(0),
            now + Duration::from_secs(2),
            || 20
        ),
        ListeningUpdate::Started
    );
    state.observe(
        Some(&song),
        Some(second),
        &playing(2000),
        now + Duration::from_secs(4),
        || panic!("same occurrence"),
    );
    state.finish(true);
    assert_eq!(state.history().events.len(), 2);
    let first = &state.history().events[0];
    let second = &state.history().events[1];
    assert_eq!(
        (first.started_at, first.listened_seconds, first.completed),
        (10, 1, false)
    );
    assert_eq!(
        (second.started_at, second.listened_seconds, second.completed),
        (20, 2, true)
    );
}

#[test]
fn missing_occurrence_identity_does_not_attribute_old_audio_to_a_selected_song() {
    let mut state = ListeningState::default();
    assert_eq!(
        state.observe(
            Some(&song("selected")),
            None,
            &playing(1000),
            Instant::now(),
            || panic!("no occurrence")
        ),
        ListeningUpdate::Unchanged
    );
    assert!(state.history().events.is_empty());
}

#[test]
fn buffering_and_failed_start_do_not_create_listening_events() {
    use crate::audio::test_transport::Fixture;

    let mut fixture = Fixture::new(PlaybackState {
        output_connected: true,
        ..playing(10_000)
    });
    let mut state = ListeningState::default();
    let song = song("next");
    let occurrence = PlaybackId::new();
    let now = Instant::now();
    fixture
        .audio
        .play("test".into(), "next".into(), None, 1.0, Default::default());
    assert_eq!(
        state.observe(
            Some(&song),
            Some(occurrence),
            &fixture.audio.progress(),
            now,
            || panic!("not connected")
        ),
        ListeningUpdate::Unchanged
    );
    fixture.publish_next(playing(0));
    assert_eq!(
        state.observe(
            Some(&song),
            Some(occurrence),
            &fixture.audio.progress(),
            now,
            || panic!("still buffering")
        ),
        ListeningUpdate::Unchanged
    );
    fixture.exit_with_error("preparation failed");
    assert_eq!(
        state.observe(
            Some(&song),
            Some(occurrence),
            &fixture.audio.progress(),
            now,
            || panic!("failed start")
        ),
        ListeningUpdate::Unchanged
    );
    assert!(state.history().events.is_empty());
    assert!(state.recent_songs().is_empty());
}

#[test]
fn short_seek_during_the_same_occurrence_does_not_add_listening_time() {
    use crate::audio::test_transport::Fixture;

    let fixture = Fixture::new(PlaybackState {
        output_connected: true,
        ..playing(20_000)
    });
    let mut state = ListeningState::default();
    let song = song("song");
    let occurrence = PlaybackId::new();
    let now = Instant::now();
    assert_eq!(
        state.observe(
            Some(&song),
            Some(occurrence),
            &fixture.audio.progress(),
            now,
            || 10
        ),
        ListeningUpdate::Started
    );
    fixture.audio.seek(Duration::from_secs(21));
    state.observe(
        Some(&song),
        Some(occurrence),
        &fixture.audio.progress(),
        now,
        || panic!("same occurrence"),
    );
    fixture.publish_next(playing(21_000));
    state.observe(
        Some(&song),
        Some(occurrence),
        &fixture.audio.progress(),
        now,
        || panic!("seek buffering"),
    );
    fixture.publish(PlaybackState {
        output_connected: true,
        ..playing(21_250)
    });
    state.observe(
        Some(&song),
        Some(occurrence),
        &fixture.audio.progress(),
        now,
        || panic!("already started"),
    );
    fixture.publish(PlaybackState {
        output_connected: true,
        ..playing(22_000)
    });
    state.observe(
        Some(&song),
        Some(occurrence),
        &fixture.audio.progress(),
        now,
        || panic!("already started"),
    );
    state.finish(false);
    assert_eq!(state.history().events.len(), 1);
    assert_eq!(state.history().events[0].listened_seconds, 1);
}

#[test]
fn a_short_seek_completed_between_polls_resets_the_listening_baseline() {
    let mut state = ListeningState::default();
    let song = song("song");
    let occurrence = PlaybackId::new();
    let now = Instant::now();
    state.observe(Some(&song), Some(occurrence), &playing(20_000), now, || 10);
    state.observe(
        Some(&song),
        Some(occurrence),
        &PlaybackState {
            seek_serial: 1,
            ..playing(21_250)
        },
        now,
        || panic!("same occurrence"),
    );
    state.observe(
        Some(&song),
        Some(occurrence),
        &PlaybackState {
            seek_serial: 1,
            ..playing(22_250)
        },
        now,
        || panic!("same occurrence"),
    );
    state.finish(false);
    assert_eq!(state.history().events[0].listened_seconds, 1);
}
