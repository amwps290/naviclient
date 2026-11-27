use super::*;

fn album(id: &str) -> Album {
    Album {
        id: id.into(),
        name: format!("Album {id}"),
        ..Default::default()
    }
}

#[test]
fn push_deduplicates_consecutive_targets_and_clears_forward_stack() {
    let mut history = NavHistory::default();
    assert!(!history.can_go_back());
    assert!(!history.can_go_forward());

    history.push(NavLocation::Albums, -120.0);
    history.push(NavLocation::Albums, -240.0);
    history.push(NavLocation::album(album("a1")), -300.0);

    assert!(history.can_go_back());
    let back1 = history.go_back(-50.0).unwrap();
    assert_eq!(back1.location, NavLocation::Albums);
    assert_eq!(back1.scroll_y, -300.0);
    assert!(history.can_go_forward());

    let back2 = history.go_back(-310.0).unwrap();
    assert_eq!(back2.location, NavLocation::Home);
    assert_eq!(back2.scroll_y, -120.0);
    assert!(!history.can_go_back());

    let fwd1 = history.go_forward(0.0).unwrap();
    assert_eq!(fwd1.location, NavLocation::Albums);
    assert_eq!(fwd1.scroll_y, -310.0);

    // Pushing a new location discards remaining forward history.
    history.push(NavLocation::Search, -40.0);
    assert!(!history.can_go_forward());
}

#[test]
fn back_stack_is_bounded_and_reset_clears_both_directions() {
    let mut history = NavHistory::default();
    for i in 0..(MAX_HISTORY_LEN + 10) {
        history.push(NavLocation::GenreDetail(format!("g{i}")), -(i as f32));
    }
    let mut count = 0;
    while history.go_back(0.0).is_some() {
        count += 1;
    }
    assert_eq!(count, MAX_HISTORY_LEN);

    history.reset();
    assert!(!history.can_go_back());
    assert!(!history.can_go_forward());
}
