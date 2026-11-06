use super::*;
use crate::models::{PlaybackQueue, QueueOrigin, Song};

fn song(title: &str) -> Song {
    Song {
        id: "duplicate".into(),
        title: title.into(),
        ..Default::default()
    }
}

fn state() -> AppState {
    AppState {
        queue: PlaybackQueue::from_snapshot(
            vec![song("First"), song("Second"), song("Third")],
            vec![
                QueueOrigin::Manual,
                QueueOrigin::DailyMix,
                QueueOrigin::Playlist,
            ],
        ),
        queue_index: Some(1),
        ..Default::default()
    }
}

#[test]
fn delayed_row_events_follow_the_original_occurrence_after_moves_and_insertions() {
    for selected in 0..3 {
        for from in 0..3 {
            for to in 0..3 {
                let mut state = state();
                let target = SongRowTarget::capture(&state, SongScope::Queue, selected).unwrap();
                let title = state.queue.song(selected).unwrap().title.clone();
                let id = state.queue.entry_id(selected).unwrap();
                state.edit_queue().move_entry(from, to);
                state
                    .edit_queue()
                    .insert_next(vec![song("Inserted"), song("Inserted twice")]);
                let (scope, index) = target.resolve(&state).unwrap();
                assert_eq!(scope, SongScope::Queue);
                assert_eq!(state.queue.entry_id(index), Some(id));
                assert_eq!(state.queue.song(index).unwrap().title, title);
                assert_eq!(target.queue_index(&state), Some(index));
            }
        }
    }
}

#[test]
fn deleting_a_delayed_row_target_removes_that_occurrence_and_remaps_the_current_index() {
    let mut state = state();
    let target = SongRowTarget::capture(&state, SongScope::Queue, 0).unwrap();
    let playing = state.queue.entry_id(1).unwrap();
    state.edit_queue().move_entry(0, 2);
    let index = target.queue_index(&state).unwrap();
    assert_eq!(index, 2);
    assert_eq!(state.queue.song(index).unwrap().title, "First");
    assert_eq!(
        state.edit_queue().remove(index),
        crate::app::queue_edits::QueueRemoval::KeptCurrent,
    );
    assert_eq!(state.queue.song_ids(), ["duplicate", "duplicate"]);
    assert_eq!(
        state.queue.entry_id(state.queue_index.unwrap()),
        Some(playing)
    );
    assert_eq!(target.resolve(&state), None);
    assert_eq!(target.queue_move(&state, -1), None);
}

#[test]
fn removing_then_reinserting_the_same_song_invalidates_every_old_row_operation() {
    let mut state = state();
    let target = SongRowTarget::capture(&state, SongScope::Queue, 1).unwrap();
    let removed = state.queue.remove(1).unwrap();
    state.queue.insert(1, vec![removed.song], removed.origin);
    assert_eq!(
        state.queue.song_ids(),
        ["duplicate", "duplicate", "duplicate"]
    );
    assert!(target.resolve(&state).is_none());
    assert!(target.queue_index(&state).is_none());
    assert!(target.queue_move(&state, -1).is_none());
    assert!(target.queue_move(&state, 1).is_none());
}

#[test]
fn restoring_a_queue_with_the_same_song_ids_cannot_reuse_an_old_row_target() {
    let mut state = state();
    let target = SongRowTarget::capture(&state, SongScope::Queue, 1).unwrap();
    let (songs, origins) = state.queue.snapshot();
    state.queue = PlaybackQueue::from_snapshot(songs, origins);
    assert_eq!(
        state.queue.song_ids(),
        ["duplicate", "duplicate", "duplicate"]
    );
    assert_eq!(target.resolve(&state), None);
    assert_eq!(target.queue_index(&state), None);
    assert_eq!(target.queue_move(&state, 1), None);
}

#[test]
fn queued_move_events_recompute_neighbors_and_reject_boundary_or_extreme_offsets() {
    let mut state = state();
    let target = SongRowTarget::capture(&state, SongScope::Queue, 1).unwrap();
    assert_eq!(target.queue_move(&state, -1), Some((1, 0)));
    assert_eq!(target.queue_move(&state, 1), Some((1, 2)));
    state.edit_queue().move_entry(1, 0);
    assert_eq!(target.queue_move(&state, -1), None);
    assert_eq!(target.queue_move(&state, 1), Some((0, 1)));
    state.edit_queue().move_entry(0, 2);
    assert_eq!(target.queue_move(&state, 1), None);
    assert_eq!(target.queue_move(&state, -1), Some((2, 1)));
    for delta in [0, isize::MIN, isize::MAX] {
        assert_eq!(target.queue_move(&state, delta), None);
    }
}

#[test]
fn library_rows_use_their_live_scoped_projection_and_cannot_edit_the_queue() {
    let mut state = state();
    let mut requests = crate::app::library_requests::LibraryRequests::default();
    let request = state.search.begin(&mut requests);
    state.search.complete(
        request,
        Ok(crate::models::SearchResults {
            songs: vec![song("Search first"), song("Search second")],
            ..Default::default()
        }),
    );
    let target = SongRowTarget::capture(&state, SongScope::Search, 1).unwrap();
    assert_eq!(target.resolve(&state), Some((SongScope::Search, 1)));
    assert_eq!(target.queue_index(&state), None);
    assert_eq!(target.queue_move(&state, -1), None);
    let request = state.search.begin(&mut requests);
    state.search.complete(
        request,
        Ok(crate::models::SearchResults {
            songs: vec![song("Only result")],
            ..Default::default()
        }),
    );
    assert_eq!(target.resolve(&state), None);
    state.search.clear();
    assert_eq!(target.resolve(&state), None);
}

#[test]
fn missing_scopes_and_invalid_rows_cannot_capture_a_target() {
    let state = state();
    for (scope, index) in [
        (SongScope::ArtistTop, 0),
        (SongScope::Queue, 3),
        (SongScope::Queue, usize::MAX),
        (SongScope::Search, 0),
        (SongScope::PlaylistDetail, 0),
    ] {
        assert!(SongRowTarget::capture(&state, scope, index).is_none());
    }
}
