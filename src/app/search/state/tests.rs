use super::*;
use crate::app::songs::selection::SongSelection;
use crate::app::{AppState, SongScope};
use crate::models::{Album, Artist, Song};

fn results(id: &str) -> SearchResults {
    SearchResults {
        artists: vec![Artist {
            id: id.into(),
            ..Default::default()
        }],
        albums: vec![Album {
            id: id.into(),
            ..Default::default()
        }],
        songs: vec![Song {
            id: id.into(),
            ..Default::default()
        }],
    }
}

fn assert_results(state: &SearchState, id: &str) {
    let results = state.results().unwrap();
    assert_eq!(results.artists.len(), 1);
    assert_eq!(results.albums.len(), 1);
    assert_eq!(results.songs.len(), 1);
    assert_eq!(results.artists[0].id, id);
    assert_eq!(results.albums[0].id, id);
    assert_eq!(results.songs[0].id, id);
}

fn selection(scope: SongScope) -> SongSelection {
    let mut selection = SongSelection::default();
    selection.toggle(scope, 0, 1);
    selection
}

#[test]
fn latest_search_replaces_all_categories_and_consumes_response_once() {
    let mut state = SearchState::default();
    let mut requests = LibraryRequests::default();
    let old = state.begin(&mut requests);
    let current = state.begin(&mut requests);
    assert_eq!(state.complete(old, Ok(results("old"))), None);
    assert_eq!(state.complete(old, Err("old failure".into())), None);
    assert!(state.loading());
    assert!(state.results().is_none());
    assert_eq!(
        state.complete(current, Ok(results("current"))),
        Some(Ok(()))
    );
    assert!(!state.loading());
    assert_results(&state, "current");
    for result in [Ok(results("duplicate")), Err("duplicate failure".into())] {
        assert_eq!(state.complete(current, result), None);
        assert_results(&state, "current");
        assert!(!state.loading());
        assert!(state.load.error().is_none());
    }
}

#[test]
fn refresh_preserves_previous_results_on_failure_and_retry_replaces_them() {
    let mut state = SearchState::default();
    let mut requests = LibraryRequests::default();
    let first = state.begin(&mut requests);
    state.complete(first, Ok(results("previous")));
    state.set_filter(SearchFilter::Albums);
    let refresh = state.begin(&mut requests);
    assert!(state.loading());
    assert_results(&state, "previous");
    assert_eq!(
        state.complete(refresh, Err("offline".into())),
        Some(Err("offline"))
    );
    assert_results(&state, "previous");
    assert!(!state.loading());
    assert_eq!(state.filter(), SearchFilter::Albums);
    assert_eq!(state.complete(refresh, Ok(results("duplicate"))), None);
    assert_eq!(state.load.error(), Some("offline"));

    let retry = state.begin(&mut requests);
    assert!(state.load.error().is_none());
    assert_eq!(
        state.complete(refresh, Err("late refresh failure".into())),
        None
    );
    assert_eq!(state.complete(retry, Ok(results("retried"))), Some(Ok(())));
    assert_results(&state, "retried");
    assert_eq!(state.filter(), SearchFilter::Albums);
}

#[test]
fn clearing_search_discards_results_and_rejects_queued_responses_after_restart() {
    let mut state = SearchState::default();
    let mut requests = LibraryRequests::default();
    let first = state.begin(&mut requests);
    state.complete(first, Ok(results("previous")));
    state.set_filter(SearchFilter::Songs);
    let pending = state.begin(&mut requests);
    state.clear();
    assert!(state.results().is_none());
    assert!(!state.loading());
    assert_eq!(state.filter(), SearchFilter::Songs);
    for result in [Ok(results("late")), Err("late failure".into())] {
        assert_eq!(state.complete(pending, result), None);
        assert!(state.results().is_none());
        assert!(state.load.error().is_none());
    }
    let current = state.begin(&mut requests);
    assert_ne!(pending, current);
    assert_eq!(state.complete(pending, Ok(results("late"))), None);
    assert!(state.loading());
    assert_eq!(
        state.complete(current, Ok(results("current"))),
        Some(Ok(()))
    );
    assert_results(&state, "current");
    state.clear();
    assert!(state.results().is_none());
    assert_eq!(state.filter(), SearchFilter::Songs);
}

#[test]
fn first_failure_and_empty_success_remain_distinct_from_a_cleared_search() {
    let mut state = SearchState::default();
    let mut requests = LibraryRequests::default();
    let first = state.begin(&mut requests);
    assert_eq!(
        state.complete(first, Err("offline".into())),
        Some(Err("offline"))
    );
    assert!(state.results().is_none());
    assert!(!state.loading());
    state.clear();
    assert!(state.load.error().is_none());
    let retry = state.begin(&mut requests);
    assert_eq!(
        state.complete(retry, Ok(SearchResults::default())),
        Some(Ok(()))
    );
    let empty = state.results().unwrap();
    assert!(empty.artists.is_empty() && empty.albums.is_empty() && empty.songs.is_empty());
    assert!(!state.loading());
    state.clear();
    assert!(state.results().is_none());
}

#[test]
fn category_changes_do_not_consume_pending_request_or_discard_results() {
    let mut state = SearchState::default();
    let mut requests = LibraryRequests::default();
    assert!(!state.set_filter(SearchFilter::All));
    let first = state.begin(&mut requests);
    for filter in [
        SearchFilter::Artists,
        SearchFilter::Albums,
        SearchFilter::Songs,
        SearchFilter::All,
    ] {
        assert!(state.set_filter(filter));
        assert!(!state.set_filter(filter));
        assert!(state.loading());
        assert!(state.results().is_none());
    }
    state.complete(first, Ok(results("all categories")));
    let refresh = state.begin(&mut requests);
    assert!(state.set_filter(SearchFilter::Songs));
    assert_results(&state, "all categories");
    assert_eq!(
        state.complete(refresh, Ok(results("refresh"))),
        Some(Ok(()))
    );
    assert_eq!(state.filter(), SearchFilter::Songs);
    assert_results(&state, "refresh");
}

#[test]
fn switching_servers_resets_filter_and_rejects_both_old_response_kinds() {
    let mut app = AppState::default();
    let mut requests = LibraryRequests::default();
    let old = app.search.begin(&mut requests);
    app.search.set_filter(SearchFilter::Artists);
    app = AppState::default();
    assert_eq!(app.search.filter(), SearchFilter::All);
    let current = app.search.begin(&mut requests);
    for result in [Ok(results("old server")), Err("old server failure".into())] {
        assert_eq!(app.complete_search(old, result), None);
        assert!(app.search.loading());
        assert!(app.search.results().is_none());
    }
    assert_eq!(
        app.complete_search(current, Ok(results("current server"))),
        Some(Ok(()))
    );
    assert_results(&app.search, "current server");
}

#[test]
fn only_accepted_success_clears_search_selection_and_projection_borrows_current_results() {
    let mut app = AppState::default();
    let mut requests = LibraryRequests::default();
    let old = app.search.begin(&mut requests);
    app.complete_search(old, Ok(results("previous")));
    app.song_selection = selection(SongScope::Search);
    let current = app.search.begin(&mut requests);
    for (request, result, expected) in [
        (old, Ok(results("old")), None),
        (old, Err("old failure".into()), None),
        (
            current,
            Err("refresh failed".into()),
            Some(Err("refresh failed".to_string())),
        ),
    ] {
        assert_eq!(app.complete_search(request, result), expected);
        assert_eq!(app.song_selection.multi().unwrap().scope, SongScope::Search);
        assert_eq!(app.song_selection.cursor(), Some((SongScope::Search, 0)));
        assert_results(&app.search, "previous");
    }
    let retry = app.search.begin(&mut requests);
    assert_eq!(
        app.complete_search(retry, Ok(results("current"))),
        Some(Ok(()))
    );
    assert!(app.song_selection.multi().is_none());
    assert_eq!(app.song_selection.cursor(), None);
    let list = app.songs_for_scope(SongScope::Search).unwrap();
    assert_eq!(list[0].id, "current");
    assert!(std::ptr::eq(
        &list[0],
        &app.search.results().unwrap().songs[0]
    ));
    app.song_selection = selection(SongScope::Search);
    assert_eq!(app.complete_search(retry, Ok(results("duplicate"))), None);
    assert!(app.song_selection.multi().is_some());

    app.song_selection = selection(SongScope::Favorites);
    let next = app.search.begin(&mut requests);
    assert_eq!(
        app.complete_search(next, Ok(SearchResults::default())),
        Some(Ok(()))
    );
    assert_eq!(
        app.song_selection.multi().unwrap().scope,
        SongScope::Favorites
    );
    assert!(app.songs_for_scope(SongScope::Search).unwrap().is_empty());
}
