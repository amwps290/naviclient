use super::*;
use crate::models::QueueEntryId;

fn state() -> AppState {
    let mut state = AppState {
        queue: PlaybackQueue::from_songs(
            (0..4)
                .map(|index| Song {
                    id: "duplicate".into(),
                    title: index.to_string(),
                    ..Default::default()
                })
                .collect(),
            QueueOrigin::Manual,
        ),
        queue_index: Some(1),
        ..Default::default()
    };
    for index in [0, 1, 3] {
        state.song_selection.toggle(SongScope::Queue, index, 4);
    }
    state
}

struct Identities {
    cursor: Option<QueueEntryId>,
    anchor: Option<QueueEntryId>,
    selected: Vec<QueueEntryId>,
}

fn identities(state: &AppState) -> Identities {
    Identities {
        cursor: state
            .song_selection
            .cursor()
            .filter(|(scope, _)| *scope == SongScope::Queue)
            .and_then(|(_, index)| state.queue.entry_id(index)),
        anchor: state
            .song_selection
            .multi()
            .filter(|multi| multi.scope == SongScope::Queue)
            .and_then(|multi| state.queue.entry_id(multi.anchor)),
        selected: state
            .song_selection
            .selected_indices(SongScope::Queue, state.queue.len())
            .map(|index| state.queue.entry_id(index).unwrap())
            .collect(),
    }
}

fn assert_surviving_identities(state: &AppState, before: &Identities) {
    let current: Vec<_> = state.queue.entries().iter().map(QueueEntry::id).collect();
    let expected: Vec<_> = current
        .iter()
        .copied()
        .filter(|id| before.selected.contains(id))
        .collect();
    let actual = identities(state);
    assert_eq!(actual.selected, expected);
    assert_eq!(
        actual.cursor,
        before.cursor.filter(|id| current.contains(id))
    );
    let expected_anchor = (!expected.is_empty()).then(|| {
        before
            .anchor
            .filter(|id| current.contains(id))
            .unwrap_or(expected[0])
    });
    assert_eq!(actual.anchor, expected_anchor);
}

#[test]
fn every_move_tracks_cursor_anchor_and_selected_occurrences_for_duplicate_songs() {
    for from in 0..4 {
        for to in 0..4 {
            let mut state = state();
            let before = identities(&state);
            state.edit_queue().move_entry(from, to);
            assert_surviving_identities(&state, &before);
        }
    }
}

#[test]
fn every_removal_drops_only_the_removed_selection_and_keeps_a_valid_range_anchor() {
    for removed in 0..4 {
        let mut state = state();
        let before = identities(&state);
        state.edit_queue().remove(removed);
        assert_surviving_identities(&state, &before);
    }
}

#[test]
fn insertion_and_rich_appends_keep_selection_on_old_entries_and_exclude_new_copies() {
    let mut state = state();
    let before = identities(&state);
    let copy = state.queue.song(1).unwrap().clone();
    state
        .edit_queue()
        .insert_next(vec![copy.clone(), copy.clone()]);
    assert_surviving_identities(&state, &before);
    state.edit_queue().append_entries([
        QueueEntry::new(copy.clone(), QueueOrigin::SongRadio),
        QueueEntry::new(copy, QueueOrigin::AutoContinue),
    ]);
    assert_surviving_identities(&state, &before);
    assert_eq!(state.queue.origin(6), Some(QueueOrigin::SongRadio));
    assert_eq!(state.queue.origin(7), Some(QueueOrigin::AutoContinue));
}

#[test]
fn trimming_and_clearing_drop_vanished_cursor_and_anchor_without_selecting_another_row() {
    let mut state = state();
    let before = identities(&state);
    assert!(state.edit_queue().trim_after_current());
    assert_surviving_identities(&state, &before);
    assert_eq!(state.song_selection.cursor(), None);
    assert_eq!(state.song_selection.multi().unwrap().anchor, 0);
    state.edit_queue().clear();
    assert_eq!(state.song_selection, SongSelection::default());
    state
        .edit_queue()
        .append(vec![Song::default()], QueueOrigin::Manual);
    assert_eq!(state.song_selection, SongSelection::default());
}

#[test]
fn queue_edits_preserve_selection_in_another_scope_even_with_a_queue_cursor() {
    let mut state = state();
    state.song_selection.select_all(SongScope::Favorites, 2);
    let multi = state.song_selection.multi().unwrap().clone();
    state.edit_queue().insert_next(vec![Song::default()]);
    assert_eq!(state.song_selection.multi(), Some(&multi));
    state.edit_queue().remove(0);
    assert_eq!(state.song_selection.multi(), Some(&multi));
    state.edit_queue().move_entry(0, 1);
    assert_eq!(state.song_selection.multi(), Some(&multi));
    state.edit_queue().clear();
    assert_eq!(state.song_selection.cursor(), None);
    assert_eq!(state.song_selection.multi(), Some(&multi));
}

#[test]
fn invalid_edits_and_empty_appends_do_not_clear_cursor_or_multi_selection() {
    let mut state = state();
    let before = state.song_selection.clone();
    assert_eq!(state.edit_queue().remove(usize::MAX), QueueRemoval::Missing);
    assert!(!state.edit_queue().move_entry(0, usize::MAX));
    assert!(!state.edit_queue().move_entry(1, 1));
    assert!(!state.edit_queue().insert_next(vec![]));
    assert!(!state.edit_queue().append_entries([]));
    assert_eq!(state.song_selection, before);
}
