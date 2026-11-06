use super::*;

fn album(id: &str) -> Album {
    Album {
        id: id.into(),
        name: format!("Album {id}"),
        ..Default::default()
    }
}

fn song(id: &str, disc: i32, track: i32) -> Song {
    Song {
        id: id.into(),
        disc: Some(disc),
        track: Some(track),
        ..Default::default()
    }
}

#[test]
fn reopening_the_same_album_rejects_old_successes_and_errors() {
    let mut details = AlbumDetails::default();
    let mut requests = LibraryRequests::default();
    details.select(album("same"));
    let old = details.begin(&mut requests).unwrap();
    details.select(album("same"));
    let current = details.begin(&mut requests).unwrap();
    assert_ne!(old, current);
    assert!(details
        .complete(old, Ok(vec![song("stale", 1, 1)]))
        .is_none());
    assert!(details.complete(old, Err("stale failure".into())).is_none());
    assert!(details.loading());
    assert!(!details.failed());
    assert!(details.songs().is_empty());
    assert!(details
        .complete(current, Ok(vec![song("current", 1, 1)]))
        .unwrap()
        .is_ok());
    assert_eq!(details.songs()[0].id, "current");
    assert!(!details.loading());
}

#[test]
fn selecting_another_album_clears_tracks_and_invalidates_requests_without_a_new_load() {
    let mut details = AlbumDetails::default();
    let mut requests = LibraryRequests::default();
    assert!(details.begin(&mut requests).is_none());
    details.select(album("A"));
    let first = details.begin(&mut requests).unwrap();
    details.complete(first, Ok(vec![song("A1", 1, 1)]));
    let old = details.begin(&mut requests).unwrap();
    details.select(album("B"));
    assert_eq!(details.album().unwrap().id, "B");
    assert!(details.songs().is_empty());
    assert!(!details.loading());
    assert!(!details.failed());
    assert!(details.complete(old, Ok(vec![song("A2", 1, 2)])).is_none());
}

#[test]
fn failure_can_be_retried_and_duplicate_responses_cannot_finish_the_retry() {
    let mut details = AlbumDetails::default();
    let mut requests = LibraryRequests::default();
    details.select(album("A"));
    let failed = details.begin(&mut requests).unwrap();
    assert_eq!(
        details
            .complete(failed, Err("offline".into()))
            .unwrap()
            .unwrap_err(),
        "offline"
    );
    assert!(details.failed());
    assert!(!details.loading());
    let retry = details.begin(&mut requests).unwrap();
    assert!(!details.failed());
    assert!(details.complete(failed, Ok(vec![])).is_none());
    assert!(details
        .complete(failed, Err("duplicate error".into()))
        .is_none());
    assert!(details.loading());
    assert!(details.complete(retry, Ok(vec![])).unwrap().is_ok());
    assert!(!details.loading());
    assert!(!details.failed());
}

#[test]
fn accepted_tracks_are_sorted_without_losing_song_metadata() {
    let mut details = AlbumDetails::default();
    let mut requests = LibraryRequests::default();
    details.select(album("A"));
    let request = details.begin(&mut requests).unwrap();
    let mut first = song("first", 1, 1);
    first.cover_art = Some("art".into());
    first.duration = Some(123);
    let songs = details
        .complete(
            request,
            Ok(vec![song("disc-two", 2, 1), song("second", 1, 2), first]),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        songs
            .iter()
            .map(|song| song.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second", "disc-two"]
    );
    assert_eq!(songs[0].cover_art.as_deref(), Some("art"));
    assert_eq!(songs[0].duration, Some(123));
    assert!(details.complete(request, Ok(vec![])).is_none());
    assert_eq!(details.songs().len(), 3);
}

#[test]
fn resetting_server_state_rejects_old_details_for_the_same_album_id() {
    let mut state = AppState::default();
    let mut requests = LibraryRequests::default();
    state.album_details.select(album("shared-id"));
    let old = state.album_details.begin(&mut requests).unwrap();
    state = AppState::default();
    state.album_details.select(album("shared-id"));
    let current = state.album_details.begin(&mut requests).unwrap();
    assert_ne!(old, current);
    assert!(state
        .album_details
        .complete(old, Ok(vec![song("old-server", 1, 1)]))
        .is_none());
    assert!(state
        .album_details
        .complete(old, Err("old-server error".into()))
        .is_none());
    assert!(state.album_details.loading());
    assert!(state
        .album_details
        .complete(current, Ok(vec![song("new-server", 1, 1)]))
        .unwrap()
        .is_ok());
    assert_eq!(state.album_details.songs()[0].id, "new-server");
}
