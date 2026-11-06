use std::time::{Duration, Instant};

use super::{frame_pair, Rotation};

#[test]
fn frame_interpolation_wraps_without_a_jump() {
    assert_eq!(frame_pair(0.0), (0, 1, 0.0));
    assert_eq!(frame_pair(1.0), (0, 1, 0.0));
    let (first, next, blend) = frame_pair(119.5 / 120.0);
    assert_eq!((first, next), (119, 0));
    assert!((blend - 0.5).abs() < 0.0001);
}

#[test]
fn long_ticks_are_capped_and_pause_preserves_angle_without_elapsed_time() {
    let mut rotation = Rotation::default();
    let start = Instant::now();
    assert!(!rotation.tick(start, true, |_, _| true));
    assert!(rotation.tick(start + Duration::from_secs(5), true, |_, _| true));
    assert!((rotation.phase() - 0.1 / 18.0).abs() < 0.00001);

    let phase = rotation.phase();
    assert!(
        !rotation.tick(start + Duration::from_secs(6), false, |_, _| {
            panic!("paused rotation must not query drawable frames")
        })
    );
    assert!(!rotation.tick(start + Duration::from_secs(20), true, |_, _| true));
    assert_eq!(rotation.phase(), phase);
    assert!(rotation.tick(start + Duration::from_millis(20_050), true, |_, _| true));
    assert!((rotation.phase() - phase - 0.05 / 18.0).abs() < 0.00001);
}

#[test]
fn changing_cover_resets_both_angle_and_clock() {
    let mut rotation = Rotation::default();
    let start = Instant::now();
    rotation.tick(start, true, |_, _| true);
    assert!(rotation.tick(start + Duration::from_millis(50), true, |_, _| true));
    assert!(rotation.phase() > 0.0);

    rotation.reset();
    assert_eq!(rotation.phase(), 0.0);
    assert!(
        !rotation.tick(start + Duration::from_secs(30), true, |_, _| {
            panic!("new cover must establish a clock before advancing")
        })
    );
    assert_eq!(rotation.phase(), 0.0);
    assert!(rotation.tick(start + Duration::from_millis(30_050), true, |_, _| true));
    assert!((rotation.phase() - 0.05 / 18.0).abs() < 0.00001);
}

#[test]
fn frame_selection_wraps_negative_and_multiple_turns() {
    for (phase, expected) in [
        (-0.25, (90, 91, 0.0)),
        (1.25, (30, 31, 0.0)),
        (2.5, (60, 61, 0.0)),
        (-1.0, (0, 1, 0.0)),
    ] {
        assert_eq!(frame_pair(phase), expected);
    }
}
