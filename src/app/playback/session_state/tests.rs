use super::*;

use crate::models::{PlayModes, QueueOrigin, RepeatMode};

const SERVER_URL: &str = "https://music.example";

fn saved_session(position: f64) -> PlaybackSession {
    PlaybackSession {
        server_url: SERVER_URL.into(),
        queue: ["first", "second"]
            .into_iter()
            .map(|id| Song {
                id: id.into(),
                duration: Some(180),
                ..Default::default()
            })
            .collect(),
        queue_origins: vec![QueueOrigin::Manual, QueueOrigin::DailyMix],
        queue_index: Some(1),
        position_secs: position,
        playback_modes: PlayModes {
            shuffle: true,
            repeat: RepeatMode::All,
        },
        shuffle_played: vec![0, 1],
        shuffle_forward: vec![0],
    }
}

fn restored_session(position: f64) -> (SessionState, AppState) {
    let mut session = SessionState::new(Instant::now());
    session.bind(Some("server-a"), Some(SERVER_URL));
    let mut state = AppState::default();
    assert!(session.restore(&mut state, saved_session(position)));
    (session, state)
}

#[test]
fn saving_before_first_play_preserves_the_restored_position_and_queue_context() {
    let (session, state) = restored_session(45.125);
    let snapshot = session
        .snapshot(&state, &crate::audio::PlaybackState::default())
        .unwrap();
    assert_eq!(snapshot.position_secs, 45.125);
    assert_eq!(snapshot.queue_index, Some(1));
    assert_eq!(
        snapshot
            .queue
            .iter()
            .map(|song| song.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert_eq!(
        snapshot.queue_origins,
        [QueueOrigin::Manual, QueueOrigin::DailyMix]
    );
    assert_eq!(snapshot.playback_modes, state.playback_modes);
    assert_eq!(snapshot.shuffle_played, [0, 1]);
    assert_eq!(snapshot.shuffle_forward, [0]);
    assert_eq!(state.now_playing.as_ref().unwrap().id, "second");
    assert!(!state.occurrence.active());
    assert!(state.occurrence.id().is_some());
}

#[test]
fn paused_seek_is_retained_until_an_accepted_start_clears_the_position() {
    let (mut session, state) = restored_session(45.0);
    session.set_position(crate::playback_timeline::PlaybackTimeline {
        position: Duration::from_secs(60),
        duration: None,
    });
    for _ in 0..2 {
        assert_eq!(
            session
                .snapshot(&state, &crate::audio::PlaybackState::default())
                .unwrap()
                .position_secs,
            60.0
        );
    }
    let display = session.display_playback(PlaybackState::default(), state.now_playing.as_ref());
    assert_eq!(display.position, Duration::from_secs(60));
    assert!(!display.active);
    assert_eq!(session.position(Duration::ZERO), Duration::from_secs(60));
    assert_eq!(session.position(Duration::ZERO), Duration::from_secs(60));
    session.clear_position();
    assert_eq!(session.position(Duration::ZERO), Duration::ZERO);
    assert_eq!(
        session
            .snapshot(
                &state,
                &crate::audio::PlaybackState {
                    position: Duration::from_secs(61),
                    ..Default::default()
                }
            )
            .unwrap()
            .position_secs,
        61.0
    );
}

#[test]
fn snapshots_keep_the_bound_server_until_the_next_connection_is_bound() {
    let (mut session, state) = restored_session(45.0);
    let mut config = crate::models::Config::default();
    let next = config.add_server();
    assert_eq!(session.server_id(), Some("server-a"));
    assert_eq!(
        session
            .snapshot(&state, &crate::audio::PlaybackState::default())
            .unwrap()
            .server_url,
        SERVER_URL
    );
    assert_ne!(session.server_id(), config.active_server_id());

    session.bind(Some(&next), Some("https://other.example"));
    assert_eq!(session.server_id(), Some(next.as_str()));
    assert_eq!(
        session
            .snapshot(&state, &crate::audio::PlaybackState::default())
            .unwrap()
            .position_secs,
        0.0
    );
    assert_eq!(
        session
            .snapshot(&state, &crate::audio::PlaybackState::default())
            .unwrap()
            .server_url,
        "https://other.example"
    );
    session.bind(None, None);
    assert!(session
        .snapshot(&state, &crate::audio::PlaybackState::default())
        .is_none());
}

#[test]
fn rejected_sessions_preserve_existing_queue_and_resume_position() {
    let (mut session, mut state) = restored_session(45.0);
    state.song_selection.toggle(SongScope::Queue, 1, 2);
    let selection = state.song_selection.clone();
    let mut wrong_server = saved_session(10.0);
    wrong_server.server_url = "https://other.example".into();
    assert!(!session.restore(&mut state, wrong_server));
    let mut empty = saved_session(10.0);
    empty.queue.clear();
    assert!(!session.restore(&mut state, empty));
    assert_eq!(state.song_selection, selection);
    assert_eq!(
        session
            .snapshot(&state, &crate::audio::PlaybackState::default())
            .unwrap()
            .position_secs,
        45.0
    );
    assert_eq!(state.queue.len(), 2);
    assert_eq!(state.queue_index, Some(1));
    assert_eq!(state.now_playing.as_ref().unwrap().id, "second");
}

#[test]
fn accepted_session_restoration_clears_queue_selection_and_preserves_other_scopes() {
    let (mut session, mut state) = restored_session(45.0);
    state.song_selection.toggle(SongScope::Queue, 1, 2);
    assert!(session.restore(&mut state, saved_session(60.0)));
    assert!(state.song_selection.multi().is_none());
    assert_eq!(state.song_selection.cursor(), None);
    state.song_selection.toggle(SongScope::Favorites, 0, 1);
    let selection = state.song_selection.clone();
    assert!(session.restore(&mut state, saved_session(70.0)));
    assert_eq!(state.song_selection, selection);
}

#[test]
fn invalid_current_index_keeps_the_queue_without_selecting_or_resuming_a_song() {
    let (mut session, mut state) = restored_session(45.0);
    let mut saved = saved_session(60.0);
    saved.queue_index = Some(usize::MAX);
    assert!(session.restore(&mut state, saved));
    assert_eq!(state.queue.len(), 2);
    assert_eq!(state.queue_index, None);
    assert!(state.now_playing.is_none());
    assert_eq!(state.occurrence.id(), None);
    assert_eq!(session.position(Duration::ZERO), Duration::ZERO);
}

#[test]
fn restoration_removes_invalid_shuffle_indices_and_keeps_valid_duplicates_in_order() {
    let (mut session, mut state) = restored_session(45.0);
    let mut saved = saved_session(60.0);
    saved.shuffle_played = vec![0, usize::MAX, 1, 0];
    saved.shuffle_forward = vec![5, 1, 0];
    assert!(session.restore(&mut state, saved));
    let snapshot = session
        .snapshot(&state, &crate::audio::PlaybackState::default())
        .unwrap();
    assert_eq!(snapshot.shuffle_played, [0, 1, 0]);
    assert_eq!(snapshot.shuffle_forward, [1, 0]);
}

#[test]
fn invalid_positions_fall_back_to_zero_without_discarding_the_queue() {
    let (mut session, mut state) = restored_session(45.0);
    for position in [-5.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300] {
        assert!(session.restore(&mut state, saved_session(position)));
        assert_eq!(state.queue.len(), 2);
        assert_eq!(
            session
                .snapshot(&state, &crate::audio::PlaybackState::default())
                .unwrap()
                .position_secs,
            0.0
        );
    }
    // A huge finite value can occur in a syntactically valid on-disk session.
    let json = serde_json::to_string(&saved_session(1e300)).unwrap();
    let decoded = serde_json::from_str(&json).unwrap();
    assert!(session.restore(&mut state, decoded));
    assert_eq!(session.position(Duration::ZERO), Duration::ZERO);
}

#[test]
fn display_clock_uses_restored_position_and_metadata_while_preserving_audio_transport() {
    let (mut session, state) = restored_session(45.0);
    let audio = PlaybackState {
        paused: true,
        buffered: Duration::from_secs(10),
        output_name: Some("device".into()),
        ..Default::default()
    };
    let display = session.display_playback(audio.clone(), state.now_playing.as_ref());
    assert!(!display.active);
    assert!(display.paused);
    assert_eq!(display.position, Duration::from_secs(45));
    assert_eq!(display.duration, Some(Duration::from_secs(180)));
    assert_eq!(display.buffered, audio.buffered);
    assert_eq!(display.output_name, audio.output_name);
    assert_eq!(audio.position, Duration::ZERO);
    assert_eq!(audio.duration, None);

    session.clear_position();
    let running = PlaybackState {
        active: true,
        position: Duration::from_secs(46),
        duration: Some(Duration::from_secs(190)),
        ..Default::default()
    };
    let display = session.display_playback(running, state.now_playing.as_ref());
    assert_eq!(display.position, Duration::from_secs(46));
    assert_eq!(display.duration, Some(Duration::from_secs(190)));
    assert!(display.active);
}

#[test]
fn playback_timeline_ignores_old_duration_before_restored_audio_starts() {
    let (session, state) = restored_session(45.0);
    let old_audio = PlaybackState {
        duration: Some(Duration::from_secs(999)),
        ..Default::default()
    };
    for (duration, expected) in [
        (Some(180), Some(Duration::from_secs(180))),
        (None, None),
        (Some(-1), None),
    ] {
        let song = Song {
            duration,
            ..state.now_playing.clone().unwrap()
        };
        let display = session.display_playback(old_audio.clone(), Some(&song));
        assert_eq!(display.position, Duration::from_secs(45));
        assert_eq!(display.duration, expected);
    }
}

#[test]
fn restored_timeline_supplies_the_same_position_and_bounds_to_all_seek_controls() {
    use crate::app::desktop::media_command_state::{absolute_seek, relative_seek, SeekAction};

    let (mut session, state) = restored_session(45.0);
    session.set_position(crate::playback_timeline::PlaybackTimeline {
        position: Duration::from_millis(60_125),
        duration: None,
    });
    let old_audio = PlaybackState {
        position: Duration::from_secs(999),
        duration: Some(Duration::from_secs(999)),
        ..Default::default()
    };
    let timeline = session
        .timeline(&old_audio, state.now_playing.as_ref())
        .unwrap();
    assert_eq!(timeline.position, Duration::from_millis(60_125));
    assert_eq!(timeline.duration, Some(Duration::from_secs(180)));
    assert_eq!(timeline.seek_seconds(5), Duration::from_millis(65_125));
    assert_eq!(
        relative_seek(Some(timeline), 5_000_000),
        SeekAction::Seek(Duration::from_millis(65_125))
    );
    assert_eq!(relative_seek(Some(timeline), 120_000_000), SeekAction::Next);
    assert_eq!(
        absolute_seek(
            Some(timeline),
            Some("/track"),
            "/track",
            Duration::from_secs(181)
        ),
        SeekAction::Ignore
    );

    assert_eq!(
        session.position(Duration::ZERO),
        Duration::from_millis(60_125)
    );
    session.clear_position();
    let connected = PlaybackState {
        active: true,
        position: Duration::from_millis(60_150),
        duration: Some(Duration::from_millis(190_123)),
        ..Default::default()
    };
    let timeline = session
        .timeline(&connected, state.now_playing.as_ref())
        .unwrap();
    assert_eq!(timeline.position, Duration::from_millis(60_150));
    assert_eq!(timeline.duration, Some(Duration::from_millis(190_123)));
}

#[test]
fn missing_song_has_no_seek_timeline_or_display_clock_but_keeps_transport_diagnostics() {
    let (session, _) = restored_session(45.0);
    let audio = PlaybackState {
        active: true,
        paused: true,
        position: Duration::from_secs(999),
        duration: Some(Duration::from_secs(999)),
        buffered: Duration::from_secs(10),
        error: Some("output unavailable".into()),
        output_name: Some("device".into()),
        ..Default::default()
    };
    assert_eq!(session.timeline(&audio, None), None);
    let display = session.display_playback(audio.clone(), None);
    assert_eq!(display.position, Duration::ZERO);
    assert_eq!(display.duration, None);
    assert!(display.active && display.paused);
    assert_eq!(display.buffered, audio.buffered);
    assert_eq!(display.error, audio.error);
    assert_eq!(display.output_name, audio.output_name);
}

#[test]
fn periodic_saves_wait_for_five_seconds_and_advance_only_after_submission() {
    let now = Instant::now();
    let mut session = SessionState::new(now);
    assert!(!session.should_save(now + Duration::from_millis(4999), true));
    assert!(session.should_save(now + SESSION_SAVE_INTERVAL, true));
    assert!(session.should_save(now + SESSION_SAVE_INTERVAL, true));
    session.submitted(now + SESSION_SAVE_INTERVAL);
    assert!(!session.should_save(now + SESSION_SAVE_INTERVAL, true));
    assert!(!session.should_save(now + Duration::from_millis(9999), true));
    assert!(session.should_save(now + Duration::from_secs(10), true));
}

#[test]
fn empty_queue_does_not_consume_a_due_save_deadline() {
    let now = Instant::now();
    let session = SessionState::new(now);
    assert!(!session.should_save(now + Duration::from_secs(20), false));
    assert!(session.should_save(now + Duration::from_secs(20), true));
}

#[test]
fn clearing_the_queue_still_produces_a_snapshot_to_replace_the_old_session() {
    let (mut session, mut state) = restored_session(45.0);
    state.queue.clear();
    state.queue_index = None;
    state.now_playing = None;
    session.clear_position();
    let snapshot = session
        .snapshot(&state, &crate::audio::PlaybackState::default())
        .unwrap();
    assert!(snapshot.queue.is_empty());
    assert!(snapshot.queue_origins.is_empty());
    assert_eq!(snapshot.queue_index, None);
    assert_eq!(snapshot.position_secs, 0.0);
}

#[test]
fn legacy_queue_origins_restore_as_manual_and_keep_the_existing_session_format() {
    let (mut session, mut state) = restored_session(45.0);
    let mut legacy = serde_json::to_value(saved_session(60.0)).unwrap();
    legacy.as_object_mut().unwrap().remove("queue_origins");
    assert!(session.restore(&mut state, serde_json::from_value(legacy).unwrap()));
    let snapshot = session
        .snapshot(&state, &crate::audio::PlaybackState::default())
        .unwrap();
    assert_eq!(
        snapshot.queue_origins,
        [QueueOrigin::Manual, QueueOrigin::Manual]
    );
    let json = serde_json::to_string(&snapshot).unwrap();
    let decoded: PlaybackSession = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.server_url, SERVER_URL);
    assert_eq!(decoded.position_secs, 60.0);
    assert_eq!(decoded.queue_index, Some(1));
}

#[test]
fn equivalent_saved_addresses_restore_the_same_queue_and_bound_server() {
    let mut session = SessionState::new(Instant::now());
    session.bind(Some("server"), Some("http://music.example/Music"));
    let mut state = AppState::default();
    for address in [
        "music.example/Music/",
        "http://MUSIC.example:80/Music/",
        " http://music.example/Music ",
    ] {
        let mut saved = saved_session(45.125);
        saved.server_url = address.into();
        assert!(session.restore(&mut state, saved));
        let snapshot = session.snapshot(&state, &PlaybackState::default()).unwrap();
        assert_eq!(snapshot.server_url, "http://music.example/Music");
        assert_eq!(snapshot.position_secs, 45.125);
        assert_eq!(state.queue.song_ids(), ["first", "second"]);
        assert_eq!(state.queue_index, Some(1));
        assert!(!state.occurrence.active());
    }
    let before = serde_json::to_value(session.snapshot(&state, &PlaybackState::default())).unwrap();
    let entries = state
        .queue
        .entries()
        .iter()
        .map(|entry| entry.id())
        .collect::<Vec<_>>();
    let occurrence = state.occurrence.id();
    for address in [
        "https://music.example/Music",
        "http://music.example/music",
        "http://other.example/Music",
        "http://music.example:8080/Music",
    ] {
        let mut saved = saved_session(1.0);
        saved.server_url = address.into();
        assert!(!session.restore(&mut state, saved));
        assert_eq!(
            serde_json::to_value(session.snapshot(&state, &PlaybackState::default())).unwrap(),
            before
        );
        assert_eq!(
            state
                .queue
                .entries()
                .iter()
                .map(|entry| entry.id())
                .collect::<Vec<_>>(),
            entries
        );
        assert_eq!(state.occurrence.id(), occurrence);
    }
}

#[test]
fn restored_positions_share_track_bounds_with_display_controls_and_saved_snapshots() {
    let (mut session, mut state) = restored_session(45.0);
    for (duration, expected) in [
        (Some(180), 180.0),
        (Some(0), 0.0),
        (None, 250.125),
        (Some(-1), 250.125),
    ] {
        let mut saved = saved_session(250.125);
        saved.queue[1].duration = duration;
        assert!(session.restore(&mut state, saved));
        let audio = PlaybackState {
            position: Duration::from_secs(999),
            duration: Some(Duration::from_secs(999)),
            ..Default::default()
        };
        let timeline = session
            .timeline(&audio, state.now_playing.as_ref())
            .unwrap();
        assert_eq!(timeline.position.as_secs_f64(), expected);
        assert_eq!(session.position(Duration::ZERO), timeline.position);
        assert_eq!(
            session
                .display_playback(audio.clone(), state.now_playing.as_ref())
                .position,
            timeline.position
        );
        let snapshot = session.snapshot(&state, &audio).unwrap();
        assert_eq!(snapshot.position_secs, expected);
        assert_eq!(snapshot.queue_index, Some(1));
        assert_eq!(
            snapshot.queue_origins,
            [QueueOrigin::Manual, QueueOrigin::DailyMix]
        );
        assert!(!state.occurrence.active());
    }
}

#[test]
fn paused_seek_uses_the_same_bound_as_subsequent_resume_and_save() {
    let (mut session, state) = restored_session(45.0);
    let timeline = session
        .timeline(&PlaybackState::default(), state.now_playing.as_ref())
        .unwrap();
    session.set_position(PlaybackTimeline {
        position: Duration::MAX,
        ..timeline
    });
    let timeline = session
        .timeline(&PlaybackState::default(), state.now_playing.as_ref())
        .unwrap();
    assert_eq!(timeline.position, Duration::from_secs(180));
    assert_eq!(timeline.seek_seconds(-5), Duration::from_secs(175));
    assert_eq!(session.position(Duration::ZERO), timeline.position);
    assert_eq!(
        session
            .snapshot(&state, &PlaybackState::default())
            .unwrap()
            .position_secs,
        180.0
    );
}

#[test]
fn active_output_duration_bounds_both_display_and_persisted_progress() {
    let (mut session, state) = restored_session(45.0);
    session.clear_position();
    for (active, output_duration, expected) in [
        (false, Some(190), 180),
        (true, Some(190), 190),
        (true, Some(170), 170),
        (true, None, 180),
        (true, Some(0), 0),
    ] {
        let playback = PlaybackState {
            active,
            paused: true,
            position: Duration::from_secs(200),
            duration: output_duration.map(Duration::from_secs),
            ..Default::default()
        };
        let timeline = session
            .timeline(&playback, state.now_playing.as_ref())
            .unwrap();
        assert_eq!(timeline.position, Duration::from_secs(expected));
        assert_eq!(
            session
                .display_playback(playback.clone(), state.now_playing.as_ref())
                .position,
            timeline.position
        );
        assert_eq!(
            session.snapshot(&state, &playback).unwrap().position_secs,
            expected as f64
        );
    }
    let no_song = AppState {
        queue: PlaybackQueue::from_songs(
            vec![Song {
                id: "queued".into(),
                ..Default::default()
            }],
            QueueOrigin::Manual,
        ),
        queue_index: Some(0),
        ..Default::default()
    };
    let old_output = PlaybackState {
        position: Duration::from_secs(200),
        ..Default::default()
    };
    assert_eq!(
        session
            .snapshot(&no_song, &old_output)
            .unwrap()
            .position_secs,
        0.0
    );
}

#[test]
fn play_resumes_a_saved_clock_but_restarts_after_a_terminal_output() {
    let (mut session, state) = restored_session(45.125);
    let song = state.now_playing.as_ref();
    let terminal = PlaybackState {
        ended: true,
        position: Duration::from_secs(180),
        duration: Some(Duration::from_secs(180)),
        ..Default::default()
    };
    assert_eq!(session.start_position(song), Duration::from_millis(45_125));
    session.clear_position();
    assert_eq!(
        session.timeline(&terminal, song).unwrap().position,
        Duration::from_secs(180)
    );
    assert_eq!(session.start_position(song), Duration::ZERO);
    let failed = PlaybackState {
        ended: false,
        error: Some("stream failed".into()),
        position: Duration::from_secs(60),
        ..terminal
    };
    assert_eq!(
        session.timeline(&failed, song).unwrap().position,
        Duration::from_secs(60)
    );
    assert_eq!(session.start_position(song), Duration::ZERO);
}
