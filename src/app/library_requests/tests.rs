use super::*;
use crate::models::{SearchResults, ServerInfo};

#[test]
fn newer_loads_ignore_old_success_and_failure() {
    let mut requests = LibraryRequests::default();
    let mut load = LibraryLoad::default();
    let old = load.begin(&mut requests);
    let current = load.begin(&mut requests);
    assert_eq!(load.complete(old, Ok(1)), None);
    assert_eq!(load.complete::<i32>(old, Err("stale error".into())), None);
    assert!(load.loading());
    assert!(load.error().is_none());
    assert_eq!(load.complete(current, Ok(2)), Some(Ok(2)));
    assert!(!load.loading());
}

#[test]
fn duplicates_preserve_current_errors_and_retry_clears_them() {
    let mut requests = LibraryRequests::default();
    let mut load = LibraryLoad::default();
    let first = load.begin(&mut requests);
    assert_eq!(
        load.complete::<i32>(first, Err("offline".into())),
        Some(Err("offline"))
    );
    assert_eq!(load.complete(first, Ok(1)), None);
    assert_eq!(load.error(), Some("offline"));
    let retry = load.begin(&mut requests);
    assert!(load.error().is_none());
    assert!(load.loading());
    assert_eq!(
        load.complete::<i32>(first, Err("duplicate error".into())),
        None
    );
    assert_eq!(load.complete(retry, Ok(2)), Some(Ok(2)));
    assert_eq!(load.complete::<i32>(retry, Err("late error".into())), None);
    assert!(load.error().is_none());
}

#[test]
fn resetting_a_pending_load_rejects_its_responses_after_restart() {
    let mut requests = LibraryRequests::default();
    let mut load = LibraryLoad::default();
    let first = load.begin(&mut requests);
    load = LibraryLoad::default();
    assert_eq!(load.complete(first, Ok(())), None);
    let second = load.begin(&mut requests);
    assert_eq!(load.complete(first, Ok(())), None);
    assert!(load.loading());
    assert_eq!(load.complete(second, Ok(())), Some(Ok(())));
}

#[test]
fn connection_library_and_search_responses_are_independent() {
    let mut state = super::super::AppState::default();
    let mut requests = LibraryRequests::default();
    let connection = state.connection.begin(&mut requests);
    let artists = state.artists.begin(&mut requests);
    let search = state.search.begin(&mut requests);
    assert!(state.artists.complete(connection, Ok(Vec::new())).is_none());
    assert!(state
        .connection
        .complete_ping(search, Ok(ServerInfo::default()), &mut requests)
        .is_none());
    assert_eq!(
        state.search.complete(artists, Ok(SearchResults::default())),
        None
    );
    assert!(state
        .artists
        .complete(artists, Ok(Vec::new()))
        .unwrap()
        .is_ok());
    assert!(state.connection.loading());
    assert!(state.search.loading());
    assert_eq!(
        state.search.complete(search, Ok(SearchResults::default())),
        Some(Ok(()))
    );
    assert!(state.connection.loading());
    assert!(state
        .connection
        .complete_ping(connection, Ok(ServerInfo::default()), &mut requests)
        .unwrap()
        .is_ok());
    assert!(!state.connection.loading());
}

#[test]
fn resetting_server_state_does_not_reuse_request_ids() {
    let mut state = super::super::AppState::default();
    let mut requests = LibraryRequests::default();
    let old_connection = state.connection.begin(&mut requests);
    let old_artists = state.artists.begin(&mut requests);
    let old_search = state.search.begin(&mut requests);
    state = super::super::AppState::default();
    let current_connection = state.connection.begin(&mut requests);
    let current_artists = state.artists.begin(&mut requests);
    let current_search = state.search.begin(&mut requests);
    assert!(state
        .connection
        .complete_ping(old_connection, Ok(ServerInfo::default()), &mut requests)
        .is_none());
    assert!(state
        .artists
        .complete(old_artists, Ok(Vec::new()))
        .is_none());
    assert_eq!(
        state
            .search
            .complete(old_search, Ok(SearchResults::default())),
        None
    );
    assert_ne!(old_connection, current_connection);
    assert_ne!(old_artists, current_artists);
    assert_ne!(old_search, current_search);
    assert!(state.connection.loading());
    assert!(state.artists.loading());
    assert!(state.search.loading());
    assert_eq!(
        state
            .search
            .complete(current_search, Ok(SearchResults::default())),
        Some(Ok(()))
    );
    assert!(state
        .artists
        .complete(current_artists, Ok(Vec::new()))
        .unwrap()
        .is_ok());
    assert!(state
        .connection
        .complete_ping(current_connection, Ok(ServerInfo::default()), &mut requests)
        .unwrap()
        .is_ok());
}
