use super::*;

#[test]
fn rapid_exit_waits_for_native_entry_then_restores_the_original_window() {
    let now = Instant::now();
    let mut state = FullscreenState::default();
    assert!(state.enter(false, now));
    assert!(!state.enter(false, now));
    assert!(!state.poll(false, now, false).toggle_native);
    assert!(!state.exit(false, now));
    assert!(!state.active());
    assert!(state.poll(true, now, false).toggle_native);
    assert!(!state.poll(true, now, false).toggle_native);
    assert!(!state.poll(false, now, false).toggle_native);
    assert!(state.native.desired.is_none());
    assert!(!state.exit(false, now));
}

#[test]
fn reentry_during_a_native_exit_keeps_the_original_restore_target() {
    let now = Instant::now();
    let mut state = FullscreenState::default();
    assert!(state.enter(false, now));
    state.poll(true, now, false);
    assert!(state.exit(true, now));
    assert!(!state.enter(true, now));
    assert!(state.poll(false, now, false).toggle_native);
    state.poll(true, now, false);
    assert!(state.exit(true, now));
    state.poll(false, now, false);
    assert!(state.native.desired.is_none());
}

#[test]
fn an_already_fullscreen_window_retains_its_native_mode_after_exit() {
    let now = Instant::now();
    let mut state = FullscreenState::default();
    assert!(!state.enter(true, now));
    assert!(!state.exit(true, now));
    assert!(!state.poll(true, now, false).toggle_native);
}

#[test]
fn external_native_exit_leaves_the_player_without_requesting_fullscreen_again() {
    let now = Instant::now();
    let mut state = FullscreenState::default();
    state.enter(false, now);
    state.poll(true, now, false);
    let update = state.poll(false, now, false);
    assert!(update.exited && update.changed);
    assert!(!update.toggle_native);
    assert!(!state.active());
}

#[test]
fn controls_fade_after_idle_and_pointer_activity_restores_them() {
    let now = Instant::now();
    let mut state = FullscreenState::default();
    state.enter(true, now);
    state.poll(true, now + IDLE_DELAY - Duration::from_millis(1), false);
    assert_eq!(state.opacity(), 1.0);
    state.poll(true, now + IDLE_DELAY + FADE_DURATION, false);
    assert_eq!(state.opacity(), 0.0);
    assert!(state.wake(now + IDLE_DELAY + FADE_DURATION));
    assert_eq!(state.opacity(), 1.0);
}

#[test]
fn keyboard_focus_and_dragging_keep_controls_visible_but_manual_hiding_persists() {
    let now = Instant::now();
    let mut state = FullscreenState::default();
    state.enter(true, now);
    state.poll(true, now + Duration::from_secs(10), true);
    assert_eq!(state.opacity(), 1.0);
    state.toggle_controls(now + Duration::from_secs(10));
    state.poll(true, now + Duration::from_secs(11), true);
    assert_eq!(state.opacity(), 0.0);
    assert!(!state.wake(now + Duration::from_secs(12)));
    assert_eq!(state.opacity(), 0.0);
    state.toggle_controls(now + Duration::from_secs(13));
    assert_eq!(state.opacity(), 1.0);
}
