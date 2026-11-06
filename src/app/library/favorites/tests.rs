use super::*;

fn key(id: &str) -> FavoriteKey {
    FavoriteKey::new(FavoriteKind::Song, id)
}

fn favorites(ids: &[&str]) -> Favorites {
    Favorites {
        songs: ids
            .iter()
            .map(|id| Song {
                id: (*id).into(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

fn load(state: &mut FavoritesState, requests: &mut LibraryRequests, items: Favorites) {
    let request = state.begin_load(requests);
    state.complete_load(request, Ok(items)).unwrap().unwrap();
}

#[test]
fn snapshot_index_keeps_song_album_and_artist_ids_distinct() {
    let mut state = FavoritesState::default();
    let mut requests = LibraryRequests::default();
    load(
        &mut state,
        &mut requests,
        Favorites {
            artists: vec![Artist {
                id: "same".into(),
                name: "Artist".into(),
                album_count: None,
                cover_art: None,
            }],
            albums: vec![Album {
                id: "same".into(),
                ..Default::default()
            }],
            songs: vec![Song {
                id: "same".into(),
                ..Default::default()
            }],
        },
    );
    assert_eq!(state.total(), 3);
    let song = state.begin_change(key("same"), &mut requests).unwrap();
    assert!(!state.is_starred(&key("same")));
    assert!(state.is_starred(&FavoriteKey::new(FavoriteKind::Album, "same")));
    assert!(state.is_starred(&FavoriteKey::new(FavoriteKind::Artist, "same")));
    state.complete_change(&song, Ok(()));
    assert!(!state.is_starred(&key("same")));
}

#[test]
fn optimistic_changes_survive_snapshots_and_block_repeat_changes_per_item() {
    let mut state = FavoritesState::default();
    let mut requests = LibraryRequests::default();
    load(&mut state, &mut requests, favorites(&["remove"]));
    let add = state.begin_change(key("add"), &mut requests).unwrap();
    let remove = state.begin_change(key("remove"), &mut requests).unwrap();
    assert!(state.begin_change(key("add"), &mut requests).is_none());
    assert!(state.begin_change(key("remove"), &mut requests).is_none());
    load(&mut state, &mut requests, favorites(&["remove", "other"]));
    assert!(state.is_starred(&key("add")));
    assert!(!state.is_starred(&key("remove")));
    assert!(state.is_starred(&key("other")));
    assert!(state.is_pending(&key("add")));
    assert!(state.is_pending(&key("remove")));
    assert_eq!(
        state.complete_change(&add, Err("add failed".into())),
        Some(Err("add failed".into()))
    );
    assert_eq!(
        state.complete_change(&remove, Err("remove failed".into())),
        Some(Err("remove failed".into()))
    );
    assert!(!state.is_starred(&key("add")));
    assert!(state.is_starred(&key("remove")));
}

#[test]
fn failure_exposes_the_latest_confirmed_snapshot() {
    let mut state = FavoritesState::default();
    let mut requests = LibraryRequests::default();
    let change = state.begin_change(key("song"), &mut requests).unwrap();
    load(&mut state, &mut requests, favorites(&["song"]));
    state.complete_change(&change, Err("acknowledgment lost".into()));
    assert!(state.is_starred(&key("song")));
    assert!(!state.is_pending(&key("song")));
}

#[test]
fn stale_snapshots_and_errors_cannot_replace_a_newer_read() {
    let mut state = FavoritesState::default();
    let mut requests = LibraryRequests::default();
    let old = state.begin_load(&mut requests);
    let current = state.begin_load(&mut requests);
    assert!(state
        .complete_load(old, Ok(favorites(&["stale"])))
        .is_none());
    assert!(state
        .complete_load(old, Err("stale error".into()))
        .is_none());
    assert!(state.loading());
    state.complete_load(current, Ok(favorites(&["current"])));
    assert!(state.is_starred(&key("current")));
    assert!(!state.is_starred(&key("stale")));
    assert!(state
        .complete_load(current, Ok(Favorites::default()))
        .is_none());
    assert_eq!(state.total(), 1);
}

#[test]
fn successful_changes_invalidate_reads_started_before_acknowledgment() {
    let mut state = FavoritesState::default();
    let mut requests = LibraryRequests::default();
    let old_read = state.begin_load(&mut requests);
    let change = state.begin_change(key("song"), &mut requests).unwrap();
    state.complete_change(&change, Ok(()));
    assert!(state.is_starred(&key("song")));
    assert!(state
        .complete_load(old_read, Ok(Favorites::default()))
        .is_none());
    assert!(state.is_starred(&key("song")));
    let refresh = state.begin_load(&mut requests);
    assert!(state
        .complete_load(old_read, Err("old failure".into()))
        .is_none());
    assert!(state.loading());
    state.complete_load(refresh, Ok(favorites(&["song"])));
    assert!(state.is_starred(&key("song")));
}

#[test]
fn duplicate_change_results_do_not_consume_a_new_change_for_the_same_key() {
    let mut state = FavoritesState::default();
    let mut requests = LibraryRequests::default();
    let add = state.begin_change(key("song"), &mut requests).unwrap();
    assert_eq!(state.complete_change(&add, Ok(())), Some(Ok(())));
    let remove = state.begin_change(key("song"), &mut requests).unwrap();
    assert!(state
        .complete_change(&add, Err("old failure".into()))
        .is_none());
    assert!(state.complete_change(&add, Ok(())).is_none());
    assert!(state.is_pending(&key("song")));
    assert!(!state.is_starred(&key("song")));
    state.complete_change(&remove, Ok(()));
    assert!(!state.is_pending(&key("song")));
    assert!(!state.is_starred(&key("song")));
}

#[test]
fn different_pending_items_finish_independently() {
    let mut state = FavoritesState::default();
    let mut requests = LibraryRequests::default();
    let first = state.begin_change(key("first"), &mut requests).unwrap();
    let second = state.begin_change(key("second"), &mut requests).unwrap();
    state.complete_change(&first, Err("first failed".into()));
    assert!(!state.is_pending(&key("first")));
    assert!(!state.is_starred(&key("first")));
    assert!(state.is_pending(&key("second")));
    assert!(state.is_starred(&key("second")));
    state.complete_change(&second, Ok(()));
    assert!(state.is_starred(&key("second")));
    assert!(!state.is_pending(&key("second")));
}

#[test]
fn server_reset_rejects_old_reads_and_changes_for_the_same_item() {
    let mut state = AppState::default();
    let mut requests = LibraryRequests::default();
    let old_read = state.favorites.begin_load(&mut requests);
    let old_change = state
        .favorites
        .begin_change(key("shared-id"), &mut requests)
        .unwrap();
    state = AppState::default();
    let current_read = state.favorites.begin_load(&mut requests);
    let current_change = state
        .favorites
        .begin_change(key("shared-id"), &mut requests)
        .unwrap();
    assert!(state
        .favorites
        .complete_load(old_read, Ok(favorites(&["old-server"])))
        .is_none());
    assert!(state
        .favorites
        .complete_change(&old_change, Err("old-server error".into()))
        .is_none());
    assert!(state
        .favorites
        .complete_change(&old_change, Ok(()))
        .is_none());
    assert!(state.favorites.loading());
    assert!(state.favorites.is_pending(&key("shared-id")));
    state
        .favorites
        .complete_load(current_read, Ok(Favorites::default()));
    state.favorites.complete_change(&current_change, Ok(()));
    assert!(state.favorites.is_starred(&key("shared-id")));
    assert!(!state.favorites.is_starred(&key("old-server")));
}

#[test]
fn read_failures_preserve_snapshot_and_pending_changes() {
    let mut state = FavoritesState::default();
    let mut requests = LibraryRequests::default();
    load(&mut state, &mut requests, favorites(&["cached"]));
    let change = state.begin_change(key("new"), &mut requests).unwrap();
    let read = state.begin_load(&mut requests);
    assert_eq!(
        state
            .complete_load(read, Err("offline".into()))
            .unwrap()
            .unwrap_err(),
        "offline"
    );
    assert_eq!(state.items().songs[0].id, "cached");
    assert!(state.is_starred(&key("cached")));
    assert!(state.is_starred(&key("new")));
    assert!(state.is_pending(&key("new")));
    state.complete_change(&change, Ok(()));
    assert!(state.is_starred(&key("new")));
}
