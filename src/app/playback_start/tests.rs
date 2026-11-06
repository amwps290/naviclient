use super::*;
use crate::audio::PlaybackState;
use crate::models::QueueOrigin;
use std::time::Instant;

fn song(id: &str) -> Song {
    Song {
        id: id.into(),
        duration: Some(180),
        ..Default::default()
    }
}

fn api() -> OpenSubsonicProvider {
    OpenSubsonicProvider::new("http://127.0.0.1:1", "fixture-user", "fixture-password").unwrap()
}

fn state() -> AppState {
    let mut state = AppState {
        queue: PlaybackQueue::from_songs(vec![song("old"), song("other")], QueueOrigin::Playlist),
        ..Default::default()
    };
    state.shuffle_history.start(0);
    select_state(
        &mut state,
        0,
        &song("old"),
        TranscodingQuality::Original,
        Selection::New,
    );
    state.occurrence.claim_report("old");
    state.occurrence.claim_scrobble("old");
    state.song_selection.toggle(SongScope::Queue, 1, 2);
    let now = Instant::now();
    state.listening.observe(
        state.now_playing.as_ref(),
        state.occurrence.id(),
        &PlaybackState {
            active: true,
            ..Default::default()
        },
        now,
        || 1,
    );
    state.listening.observe(
        state.now_playing.as_ref(),
        state.occurrence.id(),
        &PlaybackState {
            active: true,
            position: Duration::from_secs(1),
            ..Default::default()
        },
        now + Duration::from_secs(1),
        || panic!("same occurrence"),
    );
    state
}

#[derive(Debug, PartialEq)]
struct Snapshot {
    entries: Vec<QueueEntryId>,
    index: Option<usize>,
    song: Option<String>,
    quality: Option<TranscodingQuality>,
    occurrence: Option<crate::models::PlaybackId>,
    history: (Vec<usize>, Vec<usize>),
    listening: serde_json::Value,
    selection: super::super::song_selection::SongSelection,
}

fn snapshot(state: &AppState) -> Snapshot {
    Snapshot {
        entries: state.queue.entries().iter().map(QueueEntry::id).collect(),
        index: state.queue_index,
        song: state.now_playing.as_ref().map(|song| song.id.clone()),
        quality: state.now_playing_quality,
        occurrence: state.occurrence.id(),
        history: state.shuffle_history.snapshot(),
        listening: serde_json::to_value(state.listening.history()).unwrap(),
        selection: state.song_selection.clone(),
    }
}

fn session() -> SessionState {
    SessionState::new(Instant::now())
}

fn prepared(target: StartTarget, position: Duration, selection: Selection) -> PreparedStart {
    PreparedStart::prepare(
        target,
        Some(&api()),
        TranscodingQuality::Original,
        position,
        selection,
    )
    .unwrap()
}

#[test]
fn rejected_queue_replacement_preserves_current_entry_identity_history_and_reporting() {
    let mut state = state();
    let before = snapshot(&state);
    let target = StartTarget::replacement(
        PlaybackQueue::from_songs(vec![song("new")], QueueOrigin::Manual),
        0,
    )
    .unwrap();
    let mut history = ShuffleHistory::default();
    history.start(0);
    assert!(matches!(
        prepared(target, Duration::ZERO, Selection::New)
            .with_history(history)
            .commit(&mut state, &mut session(), |_| false),
        Err(StartError::OutputUnavailable)
    ));
    assert_eq!(snapshot(&state), before);
    assert!(!state.occurrence.claim_report("old"));
    assert!(!state.occurrence.claim_scrobble("old"));
    assert!(state.listening.finish(false));
    assert_eq!(state.listening.history().events[0].listened_seconds, 1);
}

#[test]
fn accepted_replacement_commits_selected_song_origin_identity_and_history_together() {
    let mut state = state();
    let old_id = state.occurrence.id();
    let queue =
        PlaybackQueue::from_songs(vec![song("first"), song("selected")], QueueOrigin::DailyMix);
    let selected_entry = queue.entry_id(1).unwrap();
    let target = StartTarget::replacement(queue, 1).unwrap();
    let mut history = ShuffleHistory::default();
    history.start(1);
    let effects = prepared(target, Duration::ZERO, Selection::New)
        .with_history(history)
        .commit(&mut state, &mut session(), |request| {
            assert_eq!(
                request.cache_key,
                "http://127.0.0.1:1:selected:profile=original"
            );
            assert_eq!(request.position, Duration::ZERO);
            true
        })
        .unwrap();
    assert_eq!(state.queue.song_ids(), ["first", "selected"]);
    assert_eq!(state.queue_index, Some(1));
    assert_eq!(state.queue.entry_id(1), Some(selected_entry));
    assert_eq!(state.queue.origin(1), Some(QueueOrigin::DailyMix));
    assert_eq!(state.now_playing.as_ref().unwrap().id, "selected");
    assert_ne!(state.occurrence.id(), old_id);
    assert_eq!(state.shuffle_history.snapshot(), (vec![1], vec![]));
    assert!(state.song_selection.cursor().is_none());
    assert!(state.song_selection.multi().is_none());
    assert!(effects.finished_listening);
    assert_eq!(state.listening.history().events[0].listened_seconds, 1);
    assert_eq!(effects.song.id, "selected");
}

#[test]
fn retry_keeps_the_existing_occurrence_and_accumulated_listening_event() {
    let mut state = state();
    let id = state.occurrence.id();
    let target = StartTarget::existing(&state.queue, 0).unwrap();
    let effects = prepared(target, Duration::from_secs(42), Selection::Resume)
        .commit(&mut state, &mut session(), |request| {
            assert_eq!(request.position, Duration::from_secs(42));
            true
        })
        .unwrap();
    assert_eq!(state.occurrence.id(), id);
    assert!(!effects.finished_listening);
    assert!(!state.occurrence.claim_report("old"));
    assert!(!state.occurrence.claim_scrobble("old"));
    assert!(state.listening.finish(false));
    assert_eq!(state.listening.history().events[0].listened_seconds, 1);
}

#[test]
fn rejected_restored_start_keeps_resume_position_until_an_accepted_retry() {
    let mut state = AppState::default();
    let mut session = session();
    session.bind(Some("fixture-server"), Some(api().base_url()));
    assert!(session.restore(
        &mut state,
        crate::models::PlaybackSession {
            server_url: api().base_url().into(),
            queue: vec![song("old")],
            queue_index: Some(0),
            position_secs: 42.125,
            ..Default::default()
        },
    ));
    let before = snapshot(&state);
    let position = Duration::from_millis(42_125);
    let target = StartTarget::existing(&state.queue, 0).unwrap();
    assert!(matches!(
        prepared(target, session.position(Duration::ZERO), Selection::Resume).commit(
            &mut state,
            &mut session,
            |request| {
                assert_eq!(request.position, position);
                false
            }
        ),
        Err(StartError::OutputUnavailable)
    ));
    assert_eq!(snapshot(&state), before);
    assert!(!state.occurrence.active());
    assert_eq!(session.position(Duration::ZERO), position);
    assert_eq!(
        session
            .snapshot(&state, &crate::audio::PlaybackState::default())
            .unwrap()
            .position_secs,
        42.125
    );

    let target = StartTarget::existing(&state.queue, 0).unwrap();
    prepared(target, session.position(Duration::ZERO), Selection::Resume)
        .commit(&mut state, &mut session, |request| {
            assert_eq!(request.position, position);
            true
        })
        .unwrap();
    assert_eq!(state.occurrence.id(), before.occurrence);
    assert!(state.occurrence.active());
    assert_eq!(
        session.position(Duration::from_secs(43)),
        Duration::from_secs(43)
    );
}

#[test]
fn restored_overlong_position_matches_display_save_and_the_accepted_play_command() {
    use crate::audio::test_transport::Fixture;
    let mut session = session();
    session.bind(Some("fixture-server"), Some(api().base_url()));
    let mut state = AppState::default();
    assert!(session.restore(
        &mut state,
        crate::models::PlaybackSession {
            server_url: format!("{}/", api().base_url()),
            queue: vec![song("old")],
            queue_index: Some(0),
            position_secs: 250.125,
            ..Default::default()
        }
    ));
    let fixture = Fixture::new(PlaybackState::default());
    let position = session
        .timeline(&fixture.audio.state(), state.now_playing.as_ref())
        .unwrap()
        .position;
    assert_eq!(position, Duration::from_secs(180));
    assert_eq!(
        session
            .snapshot(&state, &fixture.audio.state())
            .unwrap()
            .position_secs,
        180.0
    );
    let occurrence = state.occurrence.id();
    let target = StartTarget::existing(&state.queue, 0).unwrap();
    let start = prepared(target, position, Selection::Resume);
    let effects = start
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
    assert_eq!(effects.position, position);
    assert_eq!(fixture.audio.state().position, position);
    assert_eq!(fixture.commands(), ["play"]);
    assert_eq!(state.occurrence.id(), occurrence);
    assert!(state.occurrence.active());
}

#[test]
fn stale_same_song_replacement_at_the_index_is_rejected_before_sending_audio() {
    let mut state = state();
    let target = StartTarget::existing(&state.queue, 0).unwrap();
    let start = prepared(target, Duration::ZERO, Selection::New);
    state.queue.remove(0);
    state
        .queue
        .insert(0, vec![song("old")], QueueOrigin::Manual);
    let before = snapshot(&state);
    assert!(matches!(
        start.commit(&mut state, &mut session(), |_| panic!(
            "stale request must not reach audio"
        )),
        Err(StartError::Stale)
    ));
    assert_eq!(snapshot(&state), before);
}

#[test]
fn rejected_shuffle_previous_keeps_played_and_forward_history_unchanged() {
    let mut state = state();
    state.shuffle_history.advance(1);
    let before = snapshot(&state);
    let mut proposed = state.shuffle_history.clone();
    let previous = proposed.previous().unwrap();
    let target = StartTarget::existing(&state.queue, previous).unwrap();
    assert!(matches!(
        prepared(target, Duration::ZERO, Selection::New)
            .with_history(proposed)
            .commit(&mut state, &mut session(), |_| false),
        Err(StartError::OutputUnavailable)
    ));
    assert_eq!(snapshot(&state), before);
}

#[test]
fn accepted_shuffle_previous_commits_the_forward_slot_with_the_selected_entry() {
    let mut state = state();
    state.shuffle_history.advance(1);
    let mut proposed = state.shuffle_history.clone();
    let previous = proposed.previous().unwrap();
    let target = StartTarget::existing(&state.queue, previous).unwrap();
    prepared(target, Duration::ZERO, Selection::New)
        .with_history(proposed)
        .commit(&mut state, &mut session(), |_| true)
        .unwrap();
    assert_eq!(state.queue_index, Some(0));
    assert_eq!(state.shuffle_history.snapshot(), (vec![0], vec![1]));
}

fn continuation() -> Vec<QueueEntry> {
    vec![
        QueueEntry::new(song("new"), QueueOrigin::AutoContinue),
        QueueEntry::new(song("radio"), QueueOrigin::SongRadio),
    ]
}

#[test]
fn rejected_continuation_does_not_append_unplayed_entries() {
    let mut state = state();
    let before = snapshot(&state);
    let target = StartTarget::append(&state.queue, continuation()).unwrap();
    assert!(matches!(
        prepared(target, Duration::ZERO, Selection::New)
            .commit(&mut state, &mut session(), |_| false),
        Err(StartError::OutputUnavailable)
    ));
    assert_eq!(snapshot(&state), before);
}

#[test]
fn accepted_continuation_preserves_old_entries_and_selects_first_appended_occurrence() {
    let mut state = state();
    let before: Vec<_> = state.queue.entries().iter().map(QueueEntry::id).collect();
    let entries = continuation();
    let appended = entries[0].id();
    let target = StartTarget::append(&state.queue, entries).unwrap();
    prepared(target, Duration::ZERO, Selection::New)
        .commit(&mut state, &mut session(), |_| true)
        .unwrap();
    assert_eq!(state.queue.song_ids(), ["old", "other", "new", "radio"]);
    assert_eq!(state.queue.entry_id(0), Some(before[0]));
    assert_eq!(state.queue.entry_id(1), Some(before[1]));
    assert_eq!(state.queue.entry_id(2), Some(appended));
    assert_eq!(state.queue.origin(2), Some(QueueOrigin::AutoContinue));
    assert_eq!(state.queue.origin(3), Some(QueueOrigin::SongRadio));
    assert_eq!(state.queue_index, Some(2));
    assert_eq!(state.now_playing.as_ref().unwrap().id, "new");
}

#[test]
fn queue_edit_invalidates_a_prepared_continuation_before_audio_is_sent() {
    let mut state = state();
    let target = StartTarget::append(&state.queue, continuation()).unwrap();
    let start = prepared(target, Duration::ZERO, Selection::New);
    state.queue.move_entry(0, 1);
    let before = snapshot(&state);
    assert!(matches!(
        start.commit(&mut state, &mut session(), |_| panic!("changed queue")),
        Err(StartError::Stale)
    ));
    assert_eq!(snapshot(&state), before);
}

#[test]
fn missing_server_and_request_failure_never_produce_a_committable_start() {
    let state = state();
    let target = StartTarget::existing(&state.queue, 0).unwrap();
    assert!(matches!(
        PreparedStart::prepare(
            target,
            None,
            TranscodingQuality::Original,
            Duration::ZERO,
            Selection::New
        ),
        Err(StartError::NoServer)
    ));
    let target = StartTarget::replacement(
        PlaybackQueue::from_songs(vec![song("new")], QueueOrigin::Manual),
        0,
    )
    .unwrap();
    let result = PreparedStart::prepare_with(
        target,
        Some(&api()),
        TranscodingQuality::Original,
        Duration::ZERO,
        Selection::New,
        |_, _, _| Err(anyhow::anyhow!("request preparation rejected")),
    );
    assert!(matches!(result, Err(StartError::Request(_))));
}

#[test]
fn empty_and_invalid_targets_are_not_startable() {
    let state = state();
    assert!(StartTarget::existing(&state.queue, usize::MAX).is_none());
    assert!(StartTarget::replacement(PlaybackQueue::default(), 0).is_none());
    assert!(StartTarget::append(&state.queue, vec![]).is_none());
}

#[test]
fn signed_stream_profile_and_cache_identity_use_the_same_effective_quality() {
    let api = api();
    for configured in TranscodingQuality::ALL {
        for suffix in ["mp3", "OPUS"] {
            let song = Song {
                suffix: Some(suffix.into()),
                ..song("audio")
            };
            let request = PlaybackRequest::prepare(&api, &song, configured).unwrap();
            let effective = TranscodingQuality::effective_for(configured, Some(suffix), None);
            assert_eq!(request.quality, effective);
            assert_eq!(
                request.cache_key,
                format!(
                    "http://127.0.0.1:1:audio:profile={}",
                    effective.cache_profile()
                )
            );
            let url = url::Url::parse(&request.url).unwrap();
            let params: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(params.get("id").unwrap(), "audio");
            assert_eq!(
                params.get("maxBitRate").map(|value| value.as_ref()),
                effective
                    .max_bit_rate()
                    .map(|value| value.to_string())
                    .as_deref()
            );
            assert_eq!(
                params.contains_key("format"),
                effective.max_bit_rate().is_some()
            );
            assert_eq!(request.duration, Some(Duration::from_secs(180)));
        }
    }
}

#[test]
fn preparation_clamps_known_duration_but_preserves_unknown_duration_positions() {
    for (duration, expected) in [
        (Some(10), Duration::from_secs(10)),
        (None, Duration::MAX),
        (Some(-1), Duration::MAX),
    ] {
        let queue = PlaybackQueue::from_songs(
            vec![Song {
                duration,
                ..song("track")
            }],
            QueueOrigin::Manual,
        );
        let target = StartTarget::replacement(queue, 0).unwrap();
        let start = prepared(target, Duration::MAX, Selection::Resume);
        assert_eq!(start.request.position, expected);
    }
}
