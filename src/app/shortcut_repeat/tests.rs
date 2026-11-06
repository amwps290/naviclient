use super::*;

fn key(binding: &str) -> Keystroke {
    Keystroke::parse(binding).unwrap()
}

#[test]
fn first_press_is_allowed_and_repeat_boundary_is_strict_for_every_action() {
    let now = Instant::now();
    let keystroke = key("space");
    for action in ShortcutAction::ALL {
        for elapsed in [0, 1, 249, 250, 251] {
            let mut guard = ShortcutRepeatGuard::default();
            assert!(guard.allows(&keystroke, action, now));
            assert_eq!(
                guard.allows(&keystroke, action, now + Duration::from_millis(elapsed)),
                elapsed >= 250
                    || matches!(
                        action,
                        ShortcutAction::SelectPrevious | ShortcutAction::SelectNext
                    )
            );
        }
    }
}

#[test]
fn suppressed_presses_extend_the_interval_instead_of_periodically_retriggering() {
    let now = Instant::now();
    let keystroke = key("ctrl-right");
    let mut guard = ShortcutRepeatGuard::default();
    for (elapsed, allowed) in [
        (0, true),
        (200, false),
        (300, false),
        (549, false),
        (799, true),
    ] {
        assert_eq!(
            guard.allows(
                &keystroke,
                ShortcutAction::Next,
                now + Duration::from_millis(elapsed)
            ),
            allowed
        );
    }
}

#[test]
fn another_key_or_modifier_combination_breaks_the_same_key_repeat_chain() {
    let now = Instant::now();
    let mut guard = ShortcutRepeatGuard::default();
    for (elapsed, binding) in [
        (0, "right"),
        (1, "ctrl-right"),
        (2, "right"),
        (3, "left"),
        (4, "right"),
    ] {
        assert!(guard.allows(
            &key(binding),
            ShortcutAction::Next,
            now + Duration::from_millis(elapsed)
        ));
    }
    assert!(!guard.allows(
        &key("right"),
        ShortcutAction::Next,
        now + Duration::from_millis(5)
    ));
}

#[test]
fn repeat_identity_tracks_the_key_instead_of_the_configured_action_or_key_character() {
    let now = Instant::now();
    let mut guard = ShortcutRepeatGuard::default();
    let mut keystroke = key("ctrl-p");
    assert!(guard.allows(&keystroke, ShortcutAction::Previous, now));
    keystroke.key_char = Some("P".into());
    assert!(!guard.allows(
        &keystroke,
        ShortcutAction::TogglePlayback,
        now + Duration::from_millis(1)
    ));
    assert!(guard.allows(
        &keystroke,
        ShortcutAction::TogglePlayback,
        now + Duration::from_millis(251)
    ));
}

#[test]
fn selection_movement_is_exempt_but_still_updates_the_shared_repeat_clock() {
    let now = Instant::now();
    let keystroke = key("shift-up");
    let mut guard = ShortcutRepeatGuard::default();
    for (elapsed, action) in [
        (0, ShortcutAction::SelectPrevious),
        (100, ShortcutAction::SelectNext),
        (200, ShortcutAction::SelectPrevious),
    ] {
        assert!(guard.allows(&keystroke, action, now + Duration::from_millis(elapsed)));
    }
    assert!(!guard.allows(
        &keystroke,
        ShortcutAction::SelectAll,
        now + Duration::from_millis(300)
    ));
    assert!(guard.allows(
        &keystroke,
        ShortcutAction::SelectAll,
        now + Duration::from_millis(550)
    ));
}

#[test]
fn guarded_event_sequences_match_the_previous_inline_policy() {
    let now = Instant::now();
    let keys = [key("space"), key("ctrl-right"), key("shift-down")];
    for step in [0, 50, 249, 250, 251] {
        let mut guard = ShortcutRepeatGuard::default();
        let mut previous = String::new();
        let mut previous_at = now;
        let mut tick = 0;
        for action in ShortcutAction::ALL.into_iter().cycle().take(64) {
            for keystroke in [&keys[tick % keys.len()], &keys[tick % keys.len()]] {
                let at = now + Duration::from_millis(tick as u64 * step);
                let pressed = keystroke.unparse();
                let repeated = previous == pressed
                    && at.duration_since(previous_at) < Duration::from_millis(250);
                let expected = !repeated
                    || matches!(
                        action,
                        ShortcutAction::SelectPrevious | ShortcutAction::SelectNext
                    );
                previous = pressed;
                previous_at = at;
                assert_eq!(guard.allows(keystroke, action, at), expected);
                tick += 1;
            }
        }
    }
}
