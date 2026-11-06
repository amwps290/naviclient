use super::*;

#[test]
fn normalizes_volume_to_a_safe_output_range() {
    assert!((normalize_volume(0.42) - 0.42).abs() < f32::EPSILON);
    assert_eq!(normalize_volume(-0.5), 0.0);
    assert_eq!(normalize_volume(1.5), 1.0);
    assert!((normalize_volume(f32::NAN) - 0.7).abs() < f32::EPSILON);
}

#[test]
fn muting_only_changes_effective_output_volume() {
    assert!((effective_volume(0.64, false) - 0.64).abs() < f32::EPSILON);
    assert_eq!(effective_volume(0.64, true), 0.0);
}

#[test]
fn restores_a_safe_nonzero_volume_after_muting() {
    assert!((restored_volume(0.42) - 0.42).abs() < f32::EPSILON);
    assert!((restored_volume(0.0) - 0.7).abs() < f32::EPSILON);
    assert!((restored_volume(f32::NAN) - 0.7).abs() < f32::EPSILON);
}

#[test]
fn mute_preserves_saved_volume_and_an_already_pending_save() {
    let mut saved_volume = 0.64;
    let mut state = VolumeState::new(saved_volume);
    state.set_volume(&mut saved_volume, 0.42);
    let save = state.begin_save();

    assert_eq!(state.toggle_mute(&mut saved_volume), 0.0);
    assert!(state.muted());
    assert_eq!(saved_volume, 0.42);
    assert_eq!(state.output_volume(saved_volume), 0.0);
    assert!(state.complete_save(save));
    assert_eq!(saved_volume, 0.42);

    assert_eq!(state.toggle_mute(&mut saved_volume), 0.42);
    assert!(!state.muted());
    assert_eq!(saved_volume, 0.42);
    assert_eq!(state.output_volume(saved_volume), 0.42);
}

#[test]
fn setting_volume_while_muted_updates_the_volume_to_restore() {
    let mut saved_volume = 0.64;
    let mut state = VolumeState::new(saved_volume);
    state.toggle_mute(&mut saved_volume);

    assert_eq!(state.set_volume(&mut saved_volume, 0.3), 0.3);
    assert!(!state.muted());
    assert_eq!(saved_volume, 0.3);
    assert_eq!(state.output_volume(saved_volume), 0.3);

    state.toggle_mute(&mut saved_volume);
    assert_eq!(state.toggle_mute(&mut saved_volume), 0.3);
}

#[test]
fn setting_volume_to_zero_remembers_the_last_audible_volume() {
    let mut saved_volume = 0.64;
    let mut state = VolumeState::new(saved_volume);
    state.set_volume(&mut saved_volume, 0.38);
    state.set_volume(&mut saved_volume, 0.0);

    assert!(!state.muted());
    assert_eq!(saved_volume, 0.0);
    assert_eq!(state.toggle_mute(&mut saved_volume), 0.38);
    assert!(!state.muted());
    assert_eq!(saved_volume, 0.38);
}

#[test]
fn initially_silent_volume_restores_the_default() {
    let mut saved_volume = 0.0;
    let mut state = VolumeState::new(saved_volume);

    assert_eq!(state.output_volume(saved_volume), 0.0);
    assert_eq!(state.toggle_mute(&mut saved_volume), 0.7);
    assert_eq!(saved_volume, 0.7);
    assert!(!state.muted());
}

#[test]
fn rapid_volume_changes_accept_only_the_latest_save_once() {
    let mut saved_volume = 0.64;
    let mut state = VolumeState::new(saved_volume);
    state.set_volume(&mut saved_volume, 0.4);
    let first = state.begin_save();
    state.set_volume(&mut saved_volume, 0.5);
    let second = state.begin_save();
    state.set_volume(&mut saved_volume, 0.6);
    let latest = state.begin_save();

    assert!(!state.complete_save(second));
    assert!(!state.complete_save(first));
    assert!(state.complete_save(latest));
    assert_eq!(saved_volume, 0.6);
    assert!(!state.complete_save(latest));
}

#[test]
fn returning_to_panel_cancels_its_old_close_callback() {
    let mut state = VolumeState::new(0.64);
    state.show_panel();
    let first_leave = state.request_panel_close();
    state.show_panel();

    assert!(!state.complete_panel_close(first_leave));
    assert!(state.panel_open());

    let second_leave = state.request_panel_close();
    assert!(!state.complete_panel_close(first_leave));
    assert!(state.panel_open());
    assert!(state.complete_panel_close(second_leave));
    assert!(!state.panel_open());
    assert!(!state.complete_panel_close(second_leave));
}

#[test]
fn dragging_keeps_panel_open_until_release_outside_and_a_fresh_close() {
    let mut state = VolumeState::new(0.64);
    state.show_panel();
    let before_drag = state.request_panel_close();
    state.begin_panel_drag();
    assert!(!state.complete_panel_close(before_drag));

    let during_drag = state.request_panel_close();
    assert!(!state.complete_panel_close(during_drag));
    assert!(state.panel_open());

    let after_release = state
        .end_panel_drag(false)
        .expect("close after release outside");
    assert!(state.panel_open());
    assert!(!state.complete_panel_close(during_drag));
    assert!(state.complete_panel_close(after_release));
    assert!(!state.panel_open());
}

#[test]
fn releasing_inside_panel_cancels_close_scheduled_during_drag() {
    let mut state = VolumeState::new(0.64);
    state.begin_panel_drag();
    let during_drag = state.request_panel_close();

    assert!(state.end_panel_drag(true).is_none());
    assert!(!state.complete_panel_close(during_drag));
    assert!(state.panel_open());

    let next_leave = state.request_panel_close();
    assert!(state.complete_panel_close(next_leave));
    assert!(!state.panel_open());
}

#[test]
fn repeated_close_requests_wait_for_the_latest_callback() {
    let mut state = VolumeState::new(0.64);
    state.show_panel();
    let first = state.request_panel_close();
    let latest = state.request_panel_close();

    assert!(!state.complete_panel_close(first));
    assert!(state.panel_open());
    assert!(state.complete_panel_close(latest));
    assert!(!state.panel_open());
}

#[test]
fn repeated_mini_feedback_survives_the_old_expiry_callback() {
    let mut state = VolumeState::new(0.64);
    let start = Instant::now();
    let first = state.show_mini_flash(start);
    let latest = state.show_mini_flash(start + Duration::from_millis(900));

    assert!(!state.expire_mini_flash(first));
    assert!(state.mini_flash_visible(start + MINI_VOLUME_FLASH_CLEAR_DELAY));
    assert!(state.expire_mini_flash(latest));
    assert!(!state.mini_flash_visible(start + MINI_VOLUME_FLASH_CLEAR_DELAY));
    assert!(!state.expire_mini_flash(latest));
}

#[test]
fn mini_feedback_is_visible_for_one_second_before_cleanup() {
    let mut state = VolumeState::new(0.64);
    let start = Instant::now();
    assert!(!state.mini_flash_visible(start));
    let expiry = state.show_mini_flash(start);

    assert!(state.mini_flash_visible(start + Duration::from_millis(999)));
    assert!(!state.mini_flash_visible(start + MINI_VOLUME_FLASH_DURATION));
    // The later cleanup must still request a repaint when playback is paused.
    assert!(state.expire_mini_flash(expiry));
    assert!(!state.mini_flash_visible(start + MINI_VOLUME_FLASH_CLEAR_DELAY));
}

#[test]
fn save_panel_and_flash_callbacks_do_not_invalidate_each_other() {
    let mut state = VolumeState::new(0.64);
    let start = Instant::now();
    let save = state.begin_save();
    state.show_panel();
    let close = state.request_panel_close();
    let flash = state.show_mini_flash(start);

    assert!(state.expire_mini_flash(flash));
    assert!(state.panel_open());
    assert!(state.complete_panel_close(close));
    assert!(state.complete_save(save));
}
