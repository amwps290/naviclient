use super::*;
use crate::audio::test_transport::Fixture;

fn fixture(active: bool, paused: bool) -> Fixture {
    Fixture::new(PlaybackState {
        active,
        paused,
        ..Default::default()
    })
}

fn start(audio: &AudioHandle) -> bool {
    audio.play(
        "fixture".into(),
        "fixture-song".into(),
        Some(Duration::from_secs(180)),
        1.0,
        Duration::from_millis(42_125),
        None,
    )
}

fn request(audio: &AudioHandle, request: PlaybackRequest) -> PlaybackAction {
    apply_playback_request(audio, request, true, || {
        assert!(start(audio));
    })
}

#[test]
fn repeated_play_starts_once_before_the_worker_processes_any_commands() {
    let fixture = fixture(false, false);
    let audio = &fixture.audio;
    assert_eq!(request(audio, PlaybackRequest::Play), PlaybackAction::Start);
    for _ in 0..3 {
        assert_eq!(
            request(audio, PlaybackRequest::Play),
            PlaybackAction::Unchanged
        );
    }
    assert_eq!(
        request(audio, PlaybackRequest::Pause),
        PlaybackAction::Pause
    );
    assert_eq!(fixture.commands(), ["play", "pause"]);
    let playback = audio.state();
    assert!(playback.active && playback.paused);
    assert_eq!(playback.position, Duration::from_millis(42_125));
}

#[test]
fn play_after_end_or_failure_starts_at_zero_instead_of_the_worker_terminal_position() {
    use crate::app::playback::occurrence::Selection;
    use crate::app::playback::start::{PreparedStart, StartTarget};

    for ended in [false, true] {
        let (mut state, mut session) = listening_state();
        session.clear_position();
        let fixture = Fixture::new(PlaybackState {
            ended,
            error: (!ended).then(|| "stream failed".into()),
            position: Duration::from_secs(180),
            duration: Some(Duration::from_secs(180)),
            ..Default::default()
        });
        let api = MusicSource::opensubsonic(
            "fixture-server",
            "http://127.0.0.1:1",
            "fixture-user",
            "fixture-password",
        )
        .unwrap();
        assert_eq!(
            apply_playback_request(&fixture.audio, PlaybackRequest::Play, true, || {
                let position = session.start_position(state.now_playing.as_ref());
                let target =
                    StartTarget::existing(&state.queue, state.queue_index.unwrap()).unwrap();
                PreparedStart::prepare(
                    target,
                    Some(&api),
                    TranscodingQuality::Original,
                    position,
                    Selection::Resume,
                )
                .unwrap()
                .commit(&mut state, &mut session, |request| {
                    fixture.audio.play(
                        request.url.clone(),
                        request.cache_key.clone(),
                        request.duration,
                        request.gain,
                        request.position,
                        None,
                    )
                })
                .unwrap();
            }),
            PlaybackAction::Start
        );
        assert_eq!(fixture.audio.state().position, Duration::ZERO);
        assert_eq!(fixture.commands(), ["play"]);
    }
}

#[test]
fn repeated_play_from_paused_resumes_once() {
    let fixture = fixture(true, true);
    let audio = &fixture.audio;
    assert_eq!(
        request(audio, PlaybackRequest::Play),
        PlaybackAction::Resume
    );
    assert_eq!(
        request(audio, PlaybackRequest::Play),
        PlaybackAction::Unchanged
    );
    assert_eq!(
        request(audio, PlaybackRequest::Pause),
        PlaybackAction::Pause
    );
    assert_eq!(fixture.commands(), ["resume", "pause"]);
}

#[test]
fn repeated_pause_is_idempotent_and_toggle_alternates_accepted_commands() {
    let fixture = fixture(true, false);
    let audio = &fixture.audio;
    assert_eq!(
        request(audio, PlaybackRequest::Pause),
        PlaybackAction::Pause
    );
    assert_eq!(
        request(audio, PlaybackRequest::Pause),
        PlaybackAction::Unchanged
    );
    for expected in [
        PlaybackAction::Resume,
        PlaybackAction::Pause,
        PlaybackAction::Resume,
    ] {
        assert_eq!(request(audio, PlaybackRequest::Toggle), expected);
    }
    assert_eq!(fixture.commands(), ["pause", "resume", "pause", "resume"]);
}

#[test]
fn stop_then_play_starts_a_new_stream_and_pause_does_not_start_one() {
    let fixture = fixture(true, false);
    let audio = &fixture.audio;
    audio.stop();
    assert_eq!(
        request(audio, PlaybackRequest::Pause),
        PlaybackAction::Unchanged
    );
    assert_eq!(request(audio, PlaybackRequest::Play), PlaybackAction::Start);
    audio.stop();
    assert_eq!(
        request(audio, PlaybackRequest::Toggle),
        PlaybackAction::Start
    );
    assert_eq!(fixture.commands(), ["stop", "play", "stop", "play"]);
}

#[test]
fn unavailable_playback_does_not_start_or_resume() {
    for (active, paused) in [(false, false), (false, true), (true, true)] {
        let fixture = fixture(active, paused);
        let audio = &fixture.audio;
        let before = audio.state();
        for request in [PlaybackRequest::Play, PlaybackRequest::Toggle] {
            assert_eq!(
                apply_playback_request(audio, request, false, || panic!("unavailable playback")),
                PlaybackAction::Unchanged
            );
        }
        assert_eq!(audio.state(), before);
        assert!(fixture.commands().is_empty());
        assert_eq!(
            request(audio, PlaybackRequest::Play),
            if active {
                PlaybackAction::Resume
            } else {
                PlaybackAction::Start
            }
        );
    }
}

#[test]
fn active_audio_can_pause_even_when_the_server_is_unavailable() {
    let fixture = fixture(true, false);
    let audio = &fixture.audio;
    assert_eq!(
        apply_playback_request(audio, PlaybackRequest::Pause, false, || panic!(
            "pause starts audio"
        )),
        PlaybackAction::Pause
    );
    assert_eq!(
        apply_playback_request(audio, PlaybackRequest::Toggle, false, || panic!(
            "unavailable playback"
        )),
        PlaybackAction::Unchanged
    );
    assert_eq!(fixture.commands(), ["pause"]);
}

#[test]
fn rejected_preparation_does_not_suppress_a_following_play_request() {
    let fixture = fixture(false, false);
    let audio = &fixture.audio;
    let before = audio.state();
    let mut attempts = 0;
    for _ in 0..2 {
        assert_eq!(
            apply_playback_request(audio, PlaybackRequest::Play, true, || attempts += 1),
            PlaybackAction::Start
        );
        assert_eq!(audio.state(), before);
    }
    assert_eq!(attempts, 2);
    assert!(fixture.commands().is_empty());
    assert_eq!(request(audio, PlaybackRequest::Play), PlaybackAction::Start);
}

#[test]
fn rejected_delivery_does_not_invent_a_start_or_ignore_a_later_retry() {
    let mut fixture = fixture(false, false);
    fixture.disconnect();
    let audio = &fixture.audio;
    let before = audio.state();
    let mut attempts = 0;
    for _ in 0..2 {
        assert_eq!(
            apply_playback_request(audio, PlaybackRequest::Play, true, || {
                attempts += 1;
                assert!(!start(audio));
            }),
            PlaybackAction::Start
        );
        assert_eq!(audio.state(), before);
    }
    assert_eq!(attempts, 2);
    assert_eq!(
        request(audio, PlaybackRequest::Pause),
        PlaybackAction::Unchanged
    );
}

#[test]
fn rejected_resume_keeps_the_next_play_request_as_a_resume() {
    let mut fixture = fixture(true, true);
    fixture.disconnect();
    let audio = &fixture.audio;
    let before = audio.state();
    for _ in 0..2 {
        assert_eq!(
            request(audio, PlaybackRequest::Play),
            PlaybackAction::Resume
        );
        assert_eq!(audio.state(), before);
    }
    assert_eq!(
        request(audio, PlaybackRequest::Pause),
        PlaybackAction::Unchanged
    );
}

#[test]
fn rejected_pause_does_not_change_the_next_toggle_into_a_resume() {
    let mut fixture = fixture(true, false);
    fixture.disconnect();
    let audio = &fixture.audio;
    let before = audio.state();
    assert_eq!(
        request(audio, PlaybackRequest::Pause),
        PlaybackAction::Pause
    );
    assert_eq!(
        request(audio, PlaybackRequest::Toggle),
        PlaybackAction::Pause
    );
    assert_eq!(audio.state(), before);
}

#[test]
fn worker_exit_discards_a_pending_start_before_the_next_control_decision() {
    let mut fixture = fixture(false, false);
    assert_eq!(
        request(&fixture.audio, PlaybackRequest::Play),
        PlaybackAction::Start
    );
    assert_eq!(fixture.commands(), ["play"]);
    fixture.exit_with_error("output unavailable");
    let audio = &fixture.audio;
    let failed = audio.state();
    assert!(failed.worker_closed);
    assert!(!failed.active);
    assert_eq!(failed.error.as_deref(), Some("output unavailable"));
    assert_eq!(
        request(audio, PlaybackRequest::Pause),
        PlaybackAction::Unchanged
    );
    assert_eq!(
        apply_playback_request(audio, PlaybackRequest::Toggle, true, || {
            assert!(!start(audio));
        }),
        PlaybackAction::Start
    );
    assert_eq!(audio.state(), failed);
}

#[test]
fn accepted_track_selection_makes_a_following_pause_visible_without_worker_poll() {
    for (active, paused) in [(false, false), (true, true), (true, false)] {
        let fixture = fixture(active, paused);
        let audio = &fixture.audio;
        assert!(start(audio));
        assert_eq!(
            request(audio, PlaybackRequest::Play),
            PlaybackAction::Unchanged
        );
        assert_eq!(
            request(audio, PlaybackRequest::Pause),
            PlaybackAction::Pause
        );
        assert_eq!(fixture.commands(), ["play", "pause"]);
    }
}

#[test]
fn seeking_within_a_track_preserves_pause_and_a_following_play_resumes() {
    use crate::app::desktop::media_command_state::{relative_seek, SeekAction};
    use crate::playback_timeline::PlaybackTimeline;

    let fixture = fixture(true, true);
    let audio = &fixture.audio;
    let action = relative_seek(
        Some(PlaybackTimeline {
            position: Duration::from_secs(1),
            duration: Some(Duration::from_secs(10)),
        }),
        1_000_000,
    );
    let SeekAction::Seek(position) = action else {
        panic!("expected seek")
    };
    audio.seek(position);
    assert_eq!(audio.state().position, Duration::from_secs(2));
    assert_eq!(
        request(audio, PlaybackRequest::Pause),
        PlaybackAction::Unchanged
    );
    assert_eq!(
        request(audio, PlaybackRequest::Play),
        PlaybackAction::Resume
    );
    assert_eq!(fixture.commands(), ["seek", "resume"]);
}

fn listening_state() -> (AppState, SessionState) {
    use crate::app::playback::occurrence::Selection;
    use crate::app::playback::start::select_state;
    use std::time::Instant;

    let song = Song {
        id: "fixture-song".into(),
        duration: Some(180),
        ..Default::default()
    };
    let mut state = AppState {
        queue: PlaybackQueue::from_songs(vec![song.clone()], QueueOrigin::Manual),
        ..Default::default()
    };
    select_state(
        &mut state,
        0,
        &song,
        TranscodingQuality::Original,
        Selection::New,
    );
    let now = Instant::now();
    for seconds in [0, 1] {
        state.listening.observe(
            state.now_playing.as_ref(),
            state.occurrence.id(),
            &PlaybackState {
                active: true,
                position: Duration::from_secs(seconds),
                ..Default::default()
            },
            now + Duration::from_secs(seconds),
            || 1,
        );
    }
    let mut session = SessionState::new(now);
    session.bind(Some("fixture-server"), Some("http://127.0.0.1:1"));
    session.set_position(crate::playback_timeline::PlaybackTimeline {
        position: Duration::from_millis(42_125),
        duration: None,
    });
    (state, session)
}

#[test]
fn stop_finishes_partial_listening_immediately_and_retains_the_queue_for_a_fresh_start() {
    let (mut state, mut session) = listening_state();
    let entry = state.queue.entry_id(0);
    let mut requests = LibraryRequests::default();
    let lyrics = state
        .lyrics
        .begin(
            &crate::source::MediaKey::new(
                crate::source::SourceId::new(crate::source::ProviderKind::OpenSubsonic, "fixture"),
                "fixture-song",
            ),
            &mut requests,
        )
        .unwrap();
    assert!(state.lyrics.loading());
    let fixture = fixture(true, false);
    fixture.audio.stop();
    assert!(stop_playback_state(&mut state, &mut session));
    assert_eq!(state.queue.entry_id(0), entry);
    assert_eq!(state.queue_index, Some(0));
    assert!(state.now_playing.is_none());
    assert!(state.now_playing_quality.is_none());
    assert_eq!(state.occurrence.id(), None);
    assert!(!state.lyrics.loading());
    assert!(!state.lyrics.complete(lyrics, Err("late result".into())));
    assert_eq!(state.listening.history().events.len(), 1);
    let event = &state.listening.history().events[0];
    assert_eq!(event.listened_seconds, 1);
    assert!(!event.completed);
    let saved = session.snapshot(&state, &fixture.audio.state()).unwrap();
    assert_eq!(saved.queue_index, Some(0));
    assert_eq!(saved.position_secs, 0.0);
    assert!(!stop_playback_state(&mut state, &mut session));
    assert_eq!(state.listening.history().events.len(), 1);
    assert_eq!(fixture.commands(), ["stop"]);
}

#[test]
fn stop_cancels_radio_loading_and_rejects_its_late_result() {
    use crate::recommendation::RadioSeed;

    let (mut state, mut session) = listening_state();
    let song = state.now_playing.clone().unwrap();
    let mut requests = LibraryRequests::default();
    let radio = state.radio.begin(
        "fixture-server",
        &RadioSeed::Song(Box::new(song.clone())),
        &song,
        &mut requests,
    );
    assert!(state.radio.song_loading(&song.id));
    stop_playback_state(&mut state, &mut session);
    assert!(!state.radio.song_loading(&song.id));
    assert!(state
        .radio
        .complete(
            &radio,
            Some("fixture-server"),
            state.queue_index,
            &state.queue,
            state.now_playing.as_ref(),
            Err("late failure".into()),
        )
        .is_none());
    assert_eq!(state.queue.song_ids(), ["fixture-song"]);
}

#[test]
fn rejected_replacement_after_removal_stops_the_old_occurrence_and_keeps_the_surviving_queue() {
    use crate::app::playback::occurrence::Selection;
    use crate::app::playback::start::{PreparedStart, StartError, StartTarget};
    use crate::app::queue::edits::QueueRemoval;

    let (mut state, mut session) = listening_state();
    state.edit_queue().append(
        vec![Song {
            id: "replacement".into(),
            ..Default::default()
        }],
        QueueOrigin::Playlist,
    );
    let survivor = state.queue.entry_id(1);
    assert_eq!(
        state.edit_queue().remove(0),
        QueueRemoval::PlayReplacement(0)
    );
    let target = StartTarget::existing(&state.queue, 0).unwrap();
    let api = MusicSource::opensubsonic(
        "fixture-server",
        "http://127.0.0.1:1",
        "fixture-user",
        "fixture-password",
    )
    .unwrap();
    let prepared = PreparedStart::prepare(
        target,
        Some(&api),
        TranscodingQuality::Original,
        Duration::ZERO,
        Selection::New,
    )
    .unwrap();
    let mut fixture = fixture(true, false);
    fixture.disconnect();
    assert!(matches!(
        prepared.commit(&mut state, &mut session, |request| {
            fixture.audio.play(
                request.url.clone(),
                request.cache_key.clone(),
                request.duration,
                request.gain,
                request.position,
                None,
            )
        }),
        Err(StartError::OutputUnavailable)
    ));
    fixture.audio.stop();
    assert!(stop_playback_state(&mut state, &mut session));
    assert_eq!(state.queue_index, Some(0));
    assert_eq!(state.queue.entry_id(0), survivor);
    assert_eq!(state.queue.origin(0), Some(QueueOrigin::Playlist));
    assert!(state.now_playing.is_none());
    assert!(state.now_playing_quality.is_none());
    assert_eq!(state.occurrence.id(), None);
    assert_eq!(session.position(Duration::ZERO), Duration::ZERO);
    assert_eq!(state.listening.history().events.len(), 1);
    assert_eq!(state.listening.history().events[0].song.id, "fixture-song");
    assert_eq!(state.listening.history().events[0].listened_seconds, 1);
    assert!(!state.listening.history().events[0].completed);
}
