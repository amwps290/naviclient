use super::*;
use crate::models::{QueueOrigin, RepeatMode, Song};

fn queue() -> PlaybackQueue {
    PlaybackQueue::from_songs(
        ["a", "anchor"]
            .into_iter()
            .map(|id| Song {
                id: id.to_string(),
                ..Song::default()
            })
            .collect(),
        QueueOrigin::Manual,
    )
}

#[test]
fn empty_queues_cannot_start_and_pending_requests_coalesce() {
    let mut requests = LibraryRequests::default();
    let mut state = AutoContinueState::default();
    assert!(state
        .begin("server", &PlaybackQueue::default(), &mut requests)
        .is_none());
    assert!(!state.loading());
    let request = state.begin("server", &queue(), &mut requests).unwrap();
    assert!(state.begin("server", &queue(), &mut requests).is_none());
    assert!(state.loading());
    assert_eq!(state.complete(&request, Ok::<_, String>(1)), Some(Ok(1)));
    assert!(!state.loading());
}

#[test]
fn cancellation_rejects_old_results_when_the_same_context_is_restarted() {
    let mut requests = LibraryRequests::default();
    let mut state = AutoContinueState::default();
    let old = state.begin("server", &queue(), &mut requests).unwrap();
    state.cancel();
    assert!(!state.loading());
    let current = state.begin("server", &queue(), &mut requests).unwrap();
    assert_eq!(state.complete(&old, Ok::<_, String>(1)), None);
    assert_eq!(
        state.complete(&old, Err::<i32, _>("old failure".to_string())),
        None
    );
    assert!(state.loading());
    assert_eq!(state.complete(&current, Ok::<_, String>(2)), Some(Ok(2)));
}

#[test]
fn duplicate_success_or_failure_cannot_consume_a_new_request() {
    let mut requests = LibraryRequests::default();
    let mut state = AutoContinueState::default();
    let old = state.begin("server", &queue(), &mut requests).unwrap();
    assert_eq!(
        state.complete(&old, Err::<i32, _>("offline".to_string())),
        Some(Err("offline".to_string()))
    );
    assert_eq!(state.complete(&old, Ok::<_, String>(1)), None);
    let current = state.begin("server", &queue(), &mut requests).unwrap();
    assert_eq!(
        state.complete(&old, Err::<i32, _>("duplicate".to_string())),
        None
    );
    assert!(state.loading());
    assert_eq!(state.complete(&current, Ok::<_, String>(2)), Some(Ok(2)));
    assert_eq!(state.complete(&current, Ok::<_, String>(2)), None);
}

#[test]
fn server_reset_does_not_reuse_pending_request_identity() {
    let mut requests = LibraryRequests::default();
    let mut state = super::super::AppState::default();
    let old = state
        .auto_continue
        .begin("server", &queue(), &mut requests)
        .unwrap();
    state = super::super::AppState::default();
    let current = state
        .auto_continue
        .begin("server", &queue(), &mut requests)
        .unwrap();
    assert_eq!(state.auto_continue.complete(&old, ()), None);
    assert!(state.auto_continue.loading());
    assert_eq!(state.auto_continue.complete(&current, ()), Some(()));
}

#[test]
fn request_guards_server_queue_tail_length_position_and_playback_modes() {
    let mut requests = LibraryRequests::default();
    let mut state = AutoContinueState::default();
    let queue = queue();
    let request = state.begin("server", &queue, &mut requests).unwrap();
    let modes = PlayModes::default();
    assert!(request.matches(Some("server"), true, modes, Some(1), &queue));
    assert!(!request.matches(Some("other"), true, modes, Some(1), &queue));
    assert!(!request.matches(None, true, modes, Some(1), &queue));
    assert!(!request.matches(Some("server"), false, modes, Some(1), &queue));
    assert!(!request.matches(Some("server"), true, modes, Some(0), &queue));
    assert!(!request.matches(Some("server"), true, modes, None, &queue));
    assert!(!request.matches(
        Some("server"),
        true,
        PlayModes {
            shuffle: true,
            ..modes
        },
        Some(1),
        &queue
    ));
    for repeat in [RepeatMode::One, RepeatMode::All] {
        assert!(!request.matches(
            Some("server"),
            true,
            PlayModes { repeat, ..modes },
            Some(1),
            &queue
        ));
    }
    let mut changed_tail = self::queue();
    changed_tail.move_entry(0, 1);
    assert!(!request.matches(Some("server"), true, modes, Some(1), &changed_tail));
    let mut shortened = self::queue();
    shortened.remove(0);
    assert!(!request.matches(Some("server"), true, modes, Some(0), &shortened));
}
