use super::*;

#[test]
fn preparation_exposes_wait_time_and_preserves_subsecond_start_position() {
    let now = Instant::now();
    let mut state = PreparationState::default();
    assert!(!state.preparing());
    assert_eq!(state.elapsed(now), None);
    let token = state.begin(PreparationKind::Play, Duration::from_millis(45_125), now);
    assert_eq!(token.kind, PreparationKind::Play);
    assert_eq!(token.position, Duration::from_millis(45_125));
    assert!(state.preparing());
    assert_eq!(
        state.elapsed(now + Duration::from_secs(3)),
        Some(Duration::from_secs(3))
    );
    assert!(state.complete(token));
    assert!(!state.preparing());
    assert_eq!(state.elapsed(now + Duration::from_secs(4)), None);
}

#[test]
fn pause_requested_during_play_preparation_survives_completion() {
    let mut state = PreparationState::default();
    let token = state.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
    state.set_paused(true);
    assert!(state.complete(token));
    assert!(state.paused());
}

#[test]
fn seek_preserves_pause_and_a_later_resume_wins() {
    let mut state = PreparationState::default();
    state.set_paused(true);
    let token = state.begin(
        PreparationKind::Seek,
        Duration::from_secs(30),
        Instant::now(),
    );
    assert!(state.paused());
    state.set_paused(false);
    assert!(state.complete(token));
    assert!(!state.paused());
}

#[test]
fn multiple_seeks_keep_pause_intent_while_replacing_pending_position() {
    let mut state = PreparationState::default();
    state.set_paused(true);
    let first = state.begin(
        PreparationKind::Seek,
        Duration::from_secs(30),
        Instant::now(),
    );
    let latest = state.begin(
        PreparationKind::Seek,
        Duration::from_secs(60),
        Instant::now(),
    );
    assert!(!state.complete(first));
    assert!(state.paused() && state.preparing());
    assert!(state.complete(latest));
    assert!(state.paused());
    assert_eq!(latest.position, Duration::from_secs(60));
}

#[test]
fn explicitly_playing_again_restarts_intent_and_rejects_the_previous_occurrence() {
    let mut state = PreparationState::default();
    let first = state.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
    state.set_paused(true);
    let latest = state.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
    assert_ne!(first, latest);
    assert!(!state.paused());
    assert!(!state.complete(first));
    assert!(state.preparing());
    assert!(state.complete(latest));
}

#[test]
fn replacing_play_with_seek_restarts_wait_time_and_rejects_late_results() {
    let now = Instant::now();
    let mut state = PreparationState::default();
    let play = state.begin(PreparationKind::Play, Duration::ZERO, now);
    let seek = state.begin(
        PreparationKind::Seek,
        Duration::from_secs(20),
        now + Duration::from_secs(2),
    );
    assert!(!state.complete(play));
    assert_eq!(
        state.elapsed(now + Duration::from_secs(3)),
        Some(Duration::from_secs(1))
    );
    assert!(state.complete(seek));
}

#[test]
fn stop_invalidates_preparation_and_clears_wait_and_pause_intent() {
    for kind in [PreparationKind::Play, PreparationKind::Seek] {
        let now = Instant::now();
        let mut state = PreparationState::default();
        let token = state.begin(kind, Duration::from_secs(20), now);
        state.set_paused(true);
        state.stop();
        assert!(!state.preparing());
        assert!(!state.paused());
        assert_eq!(state.elapsed(now + Duration::from_secs(10)), None);
        assert!(!state.complete(token));
    }
}

#[test]
fn a_result_cannot_complete_twice_or_complete_a_later_request() {
    let now = Instant::now();
    let mut state = PreparationState::default();
    let first = state.begin(PreparationKind::Play, Duration::ZERO, now);
    assert!(state.complete(first));
    assert!(!state.complete(first));
    let later = state.begin(PreparationKind::Seek, Duration::from_secs(30), now);
    assert!(!state.complete(first));
    assert!(state.preparing());
    assert!(state.complete(later));
}

#[test]
fn completion_requires_matching_kind_and_position_in_addition_to_generation() {
    let now = Instant::now();
    let mut state = PreparationState::default();
    let token = state.begin(PreparationKind::Play, Duration::from_secs(30), now);
    for wrong in [
        PreparationToken {
            kind: PreparationKind::Seek,
            ..token
        },
        PreparationToken {
            position: Duration::from_secs(40),
            ..token
        },
    ] {
        assert!(!state.complete(wrong));
        assert!(state.preparing());
    }
    assert!(state.complete(token));
}

#[test]
fn generation_wrap_still_rejects_old_and_stopped_preparations() {
    let mut state = PreparationState {
        generation: u64::MAX - 1,
        ..Default::default()
    };
    let before_wrap = state.begin(PreparationKind::Play, Duration::ZERO, Instant::now());
    let after_wrap = state.begin(PreparationKind::Seek, Duration::ZERO, Instant::now());
    assert_eq!(before_wrap.generation, u64::MAX);
    assert_eq!(after_wrap.generation, 0);
    assert!(!state.complete(before_wrap));
    state.stop();
    assert_eq!(state.generation(), 1);
    assert!(!state.complete(after_wrap));
}

#[test]
fn clock_observation_before_begin_is_saturated_without_ending_the_wait() {
    let now = Instant::now();
    let mut state = PreparationState::default();
    let token = state.begin(
        PreparationKind::Play,
        Duration::ZERO,
        now + Duration::from_secs(1),
    );
    assert_eq!(state.elapsed(now), Some(Duration::ZERO));
    assert!(state.preparing());
    assert!(state.complete(token));
}
