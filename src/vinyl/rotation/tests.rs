use std::time::{Duration, Instant};

use super::Rotation;

#[test]
fn long_ticks_are_capped_and_pause_preserves_angle_without_elapsed_time() {
    let mut rotation = Rotation::default();
    let start = Instant::now();
    assert!(!rotation.tick(start, true, || true));
    assert!(rotation.tick(start + Duration::from_secs(5), true, || true));
    assert!((rotation.phase() - 0.1 / 18.0).abs() < 0.00001);

    let phase = rotation.phase();
    assert!(!rotation.tick(start + Duration::from_secs(6), false, || {
        panic!("paused rotation must not query drawable frames")
    }));
    assert!(!rotation.tick(start + Duration::from_secs(20), true, || true));
    assert_eq!(rotation.phase(), phase);
    assert!(rotation.tick(start + Duration::from_millis(20_050), true, || true));
    assert!((rotation.phase() - phase - 0.05 / 18.0).abs() < 0.00001);
}

#[test]
fn changing_cover_resets_both_angle_and_clock() {
    let mut rotation = Rotation::default();
    let start = Instant::now();
    rotation.tick(start, true, || true);
    assert!(rotation.tick(start + Duration::from_millis(50), true, || true));
    assert!(rotation.phase() > 0.0);

    rotation.reset();
    assert_eq!(rotation.phase(), 0.0);
    assert!(!rotation.tick(start + Duration::from_secs(30), true, || {
        panic!("new cover must establish a clock before advancing")
    }));
    assert_eq!(rotation.phase(), 0.0);
    assert!(rotation.tick(start + Duration::from_millis(30_050), true, || true));
    assert!((rotation.phase() - 0.05 / 18.0).abs() < 0.00001);
}
