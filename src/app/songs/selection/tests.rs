use super::*;

fn selected(selection: &SongSelection, scope: SongScope) -> Vec<usize> {
    selection.selected_indices(scope, 10).collect()
}

#[test]
fn ordinary_selection_clears_multi_and_invalid_rows_leave_the_previous_selection() {
    let mut selection = SongSelection::default();
    selection.toggle(SongScope::Queue, 1, 4);
    selection.select(SongScope::Queue, 2, 4);
    assert_eq!(selection.cursor(), Some((SongScope::Queue, 2)));
    assert!(selection.multi().is_none());
    let before = selection.clone();
    selection.select(SongScope::Search, usize::MAX, 4);
    assert_eq!(selection, before);
}

#[test]
fn toggles_keep_cursor_and_anchor_together_and_exit_multi_when_the_set_becomes_empty() {
    let mut selection = SongSelection::default();
    selection.toggle(SongScope::Queue, 1, 4);
    selection.toggle(SongScope::Queue, 3, 4);
    assert_eq!(selected(&selection, SongScope::Queue), [1, 3]);
    assert_eq!(selection.cursor(), Some((SongScope::Queue, 3)));
    assert_eq!(selection.multi().unwrap().anchor, 3);
    selection.toggle(SongScope::Queue, 3, 4);
    assert_eq!(selected(&selection, SongScope::Queue), [1]);
    selection.toggle(SongScope::Queue, 1, 4);
    assert!(selection.multi().is_none());
    assert_eq!(selection.cursor(), Some((SongScope::Queue, 1)));
    selection.toggle(SongScope::Favorites, 0, 1);
    assert!(selection.contains(SongScope::Favorites, 0));
    assert!(!selection.contains(SongScope::Queue, 0));
}

#[test]
fn repeated_shift_clicks_and_keyboard_ranges_keep_a_fixed_anchor_and_contract_after_folding() {
    let mut selection = SongSelection::default();
    selection.select(SongScope::Queue, 2, 6);
    selection.range(SongScope::Queue, 4, 6);
    assert_eq!(selected(&selection, SongScope::Queue), [2, 3, 4]);
    selection.range(SongScope::Queue, 0, 6);
    assert_eq!(selected(&selection, SongScope::Queue), [0, 1, 2]);
    assert_eq!(selection.multi().unwrap().anchor, 2);
    assert_eq!(selection.move_cursor(SongScope::Queue, 1, 6), Some(1));
    assert_eq!(selected(&selection, SongScope::Queue), [1, 2]);
    assert_eq!(selection.move_cursor(SongScope::Queue, 3, 6), Some(4));
    assert_eq!(selected(&selection, SongScope::Queue), [2, 3, 4]);
    assert_eq!(selection.move_cursor(SongScope::Queue, -1, 6), Some(3));
    assert_eq!(selected(&selection, SongScope::Queue), [2, 3]);
    assert_eq!(selection.multi().unwrap().anchor, 2);
}

#[test]
fn entering_another_scope_uses_its_own_range_anchor_and_cursor_navigation_is_bounded() {
    let mut selection = SongSelection::default();
    selection.range(SongScope::Queue, 4, 5);
    selection.range(SongScope::Search, 1, 3);
    assert_eq!(selected(&selection, SongScope::Search), [1]);
    assert_eq!(selection.multi().unwrap().anchor, 1);
    selection.clear_multi();
    assert_eq!(
        selection.move_cursor(SongScope::Search, isize::MAX, 3),
        Some(2)
    );
    assert_eq!(
        selection.move_cursor(SongScope::Search, isize::MIN, 3),
        Some(0)
    );
    assert!(selection.multi().is_none());
    assert_eq!(selection.move_cursor(SongScope::Favorites, -1, 4), Some(3));
    assert_eq!(selection.move_cursor(SongScope::Queue, 1, 5), Some(0));
}

#[test]
fn select_all_keeps_the_cursor_and_scope_clearing_does_not_discard_another_lists_selection() {
    let mut selection = SongSelection::default();
    selection.select(SongScope::Queue, 2, 4);
    selection.select_all(SongScope::Favorites, 3);
    assert_eq!(selection.cursor(), Some((SongScope::Queue, 2)));
    assert_eq!(selected(&selection, SongScope::Favorites), [0, 1, 2]);
    selection.clear_scope(SongScope::Queue);
    assert_eq!(selection.cursor(), None);
    assert_eq!(selected(&selection, SongScope::Favorites), [0, 1, 2]);
    selection.clear_scope(SongScope::Search);
    assert_eq!(selected(&selection, SongScope::Favorites), [0, 1, 2]);
    selection.clear_scope(SongScope::Favorites);
    assert_eq!(selection, SongSelection::default());
}

#[test]
fn empty_lists_and_stale_click_indices_cannot_create_or_extend_selection() {
    let mut selection = SongSelection::default();
    selection.toggle(SongScope::Queue, 1, 3);
    let before = selection.clone();
    for index in [3, usize::MAX] {
        selection.toggle(SongScope::Queue, index, 3);
        selection.range(SongScope::Queue, index, 3);
        selection.select(SongScope::Queue, index, 3);
        assert_eq!(selection, before);
    }
    selection.select_all(SongScope::Queue, 0);
    selection.toggle(SongScope::Queue, 0, 0);
    selection.range(SongScope::Queue, 0, 0);
    assert_eq!(selection.move_cursor(SongScope::Queue, 1, 0), None);
    assert_eq!(selection, before);
    assert_eq!(selection.selected_indices(SongScope::Queue, 1).count(), 0);
}

#[test]
fn removing_the_cursor_and_anchor_preserves_survivors_and_chooses_a_valid_anchor() {
    let mut selection = SongSelection::default();
    selection.toggle(SongScope::Queue, 1, 4);
    selection.toggle(SongScope::Queue, 3, 4);
    selection.remap_scope(SongScope::Queue, 4, 3, |index| {
        (index != 3).then_some(index)
    });
    assert_eq!(selection.cursor(), None);
    assert_eq!(selected(&selection, SongScope::Queue), [1]);
    assert_eq!(selection.multi().unwrap().anchor, 1);
    selection.move_cursor(SongScope::Queue, -1, 3);
    assert_eq!(selected(&selection, SongScope::Queue), [1, 2]);
    selection.remap_scope(SongScope::Queue, 3, 0, |_| None);
    assert_eq!(selection, SongSelection::default());
}

#[test]
fn growing_a_list_does_not_revive_an_old_out_of_bounds_cursor_or_selected_index() {
    let mut selection = SongSelection {
        cursor: Some((SongScope::Queue, 3)),
        multi: Some(MultiSelection {
            scope: SongScope::Queue,
            anchor: 3,
            indices: BTreeSet::from([0, 3]),
        }),
    };
    selection.remap_scope(SongScope::Queue, 1, 4, Some);
    assert_eq!(selection.cursor(), None);
    assert_eq!(selected(&selection, SongScope::Queue), [0]);
    assert_eq!(selection.multi().unwrap().anchor, 0);
}

#[test]
fn stale_anchors_after_a_list_shrinks_cannot_build_an_unbounded_range() {
    let mut selection = SongSelection::default();
    selection.toggle(SongScope::Queue, 9, 10);
    selection.range(SongScope::Queue, 0, 2);
    assert_eq!(selected(&selection, SongScope::Queue), [0]);
    assert_eq!(selection.multi().unwrap().anchor, 0);
    selection.toggle(SongScope::Queue, 1, 2);
    assert_eq!(selected(&selection, SongScope::Queue), [0, 1]);
}
