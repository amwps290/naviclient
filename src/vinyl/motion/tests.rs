use super::*;

fn motion_at(start: Instant, angle: f32) -> TurntableMotion {
    TurntableMotion {
        now: start,
        arm_degrees: angle,
        target_degrees: angle,
        exchange: None,
    }
}

#[test]
fn record_waits_for_the_arm_to_lift_and_arm_waits_for_the_new_record() {
    let start = Instant::now();
    let mut motion = motion_at(start, 30.0);
    assert!(motion.begin_exchange());
    motion.tick(start + LIFT / 2, true, 14.0);
    let lifting = motion.frame();
    assert_eq!(lifting.record, RecordPose::default());
    assert!(lifting.outgoing);
    assert!((lifting.arm_degrees - 15.0).abs() < 0.001);

    motion.tick(start + LIFT + REMOVE / 2, true, 14.0);
    let removing = motion.frame();
    assert!(removing.outgoing);
    assert!(removing.record.x < 0.0);
    assert!(removing.record.opacity > 0.0 && removing.record.opacity < 1.0);
    assert_eq!(removing.arm_degrees, 0.0);

    motion.tick(start + LIFT + REMOVE, true, 14.0);
    let entering = motion.frame();
    assert!(!entering.outgoing);
    assert_eq!(entering.record.opacity, 0.0);
    assert!(entering.record.x > 0.0);
    assert_eq!(entering.arm_degrees, 0.0);

    motion.tick(start + LIFT + REMOVE + INSERT, true, 14.0);
    assert_eq!(motion.frame().record, RecordPose::default());
    assert_eq!(motion.frame().arm_degrees, 0.0);

    motion.tick(start + LIFT + REMOVE + INSERT + LOWER / 2, true, 14.0);
    assert!((motion.frame().arm_degrees - 7.0).abs() < 0.001);
    motion.tick(start + LIFT + REMOVE + INSERT + LOWER, true, 14.0);
    assert!(!motion.exchanging());
    assert_eq!(motion.frame().record, RecordPose::default());
    assert_eq!(motion.frame().arm_degrees, 14.0);
}

#[test]
fn repeated_skips_during_removal_keep_the_visible_old_record_and_deadline() {
    let start = Instant::now();
    let mut motion = motion_at(start, 30.0);
    motion.begin_exchange();
    motion.tick(start + LIFT + REMOVE / 2, true, 14.0);
    let before = motion.frame();
    assert!(!motion.begin_exchange());
    assert_eq!(motion.frame(), before);
    motion.tick(start + LIFT + REMOVE, true, 14.0);
    assert!(!motion.frame().outgoing);
}

#[test]
fn skip_during_insertion_removes_the_partially_inserted_record_without_jumping() {
    let start = Instant::now();
    let mut motion = motion_at(start, 30.0);
    motion.begin_exchange();
    motion.tick(start + LIFT + REMOVE + INSERT / 2, true, 14.0);
    let before = motion.frame();
    assert!(motion.begin_exchange());
    let after = motion.frame();
    assert!(after.outgoing);
    assert_eq!(after.record, before.record);
    assert_eq!(after.arm_degrees, before.arm_degrees);
}

#[test]
fn pause_during_exchange_keeps_the_arm_parked_when_the_record_arrives() {
    let start = Instant::now();
    let mut motion = motion_at(start, 30.0);
    motion.begin_exchange();
    motion.tick(start + LIFT + REMOVE + INSERT + LOWER / 2, true, 0.0);
    assert_eq!(motion.frame().arm_degrees, 0.0);
    motion.tick(start + LIFT + REMOVE + INSERT + LOWER, true, 0.0);
    assert!(!motion.exchanging());
    assert_eq!(motion.frame().arm_degrees, 0.0);
}

#[test]
fn hiding_cancels_the_exchange_and_does_not_replay_it_on_return() {
    let start = Instant::now();
    let mut motion = motion_at(start, 30.0);
    motion.begin_exchange();
    motion.tick(start + LIFT, false, 20.0);
    assert!(!motion.exchanging());
    assert!(!motion.frame().outgoing);
    assert_eq!(motion.frame().record, RecordPose::default());
    assert!(!motion.tick(start + Duration::from_secs(10), true, 20.0));
}

#[test]
fn seeks_and_reversed_seeks_move_from_the_current_angle_then_settle() {
    let start = Instant::now();
    let mut motion = motion_at(start, 14.0);
    motion.tick(start, true, 37.0);
    assert_eq!(motion.frame().arm_degrees, 14.0);
    motion.tick(start + Duration::from_millis(50), true, 37.0);
    let forward = motion.frame().arm_degrees;
    assert!(forward > 14.0 && forward < 37.0);
    motion.tick(start + Duration::from_millis(100), true, 14.0);
    let backward = motion.frame().arm_degrees;
    assert!(backward > 14.0 && backward < forward);
    for index in 3..30 {
        motion.tick(start + Duration::from_millis(index * 50), true, 14.0);
    }
    assert_eq!(motion.frame().arm_degrees, 14.0);
    assert!(!motion.tick(start + Duration::from_secs(2), true, 14.0));
}

#[test]
fn progress_is_bounded_and_unknown_duration_stays_at_the_outer_groove() {
    let duration = Some(Duration::from_secs(100));
    assert_eq!(tonearm_target(true, Duration::ZERO, duration), 10.5);
    assert_eq!(
        tonearm_target(true, Duration::from_secs(50), duration),
        18.5
    );
    assert_eq!(
        tonearm_target(true, Duration::from_secs(100), duration),
        26.5
    );
    assert_eq!(tonearm_target(true, Duration::MAX, duration), 26.5);
    assert_eq!(tonearm_target(true, Duration::MAX, None), 10.5);
    assert_eq!(
        tonearm_target(true, Duration::MAX, Some(Duration::ZERO)),
        10.5
    );
    assert_eq!(
        tonearm_target(false, Duration::from_secs(50), duration),
        0.0
    );
}

#[test]
fn stylus_travels_inward_and_remains_on_the_grooves_outside_the_cover_label() {
    let mut previous_radius = f32::MAX;
    // Stylus tip after the SVG's local +12° headshell transform around (120, 185),
    // relative to its (110, 110) pivot. The rendered arm is 1.42 record diameters wide.
    let tip = (
        1.42 * (116.777 - 110.0) / 220.0,
        1.42 * (200.161 - 110.0) / 220.0,
    );
    for position in 0..=100 {
        let angle = tonearm_target(
            true,
            Duration::from_secs(position),
            Some(Duration::from_secs(100)),
        )
        .to_radians();
        let x = 0.97 + tip.0 * angle.cos() - tip.1 * angle.sin() - 0.5;
        let y = 0.065 + tip.0 * angle.sin() + tip.1 * angle.cos() - 0.5;
        let radius = x.hypot(y);
        assert!(radius < previous_radius);
        assert!(radius > 0.25 && radius < 0.48);
        previous_radius = radius;
    }
}
