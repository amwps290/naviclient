use super::*;

fn artist(id: &str) -> Artist {
    Artist {
        id: id.into(),
        name: id.into(),
        album_count: None,
        cover_art: None,
    }
}

fn album(id: &str) -> Album {
    Album {
        id: id.into(),
        ..Default::default()
    }
}

fn song(id: &str) -> Song {
    Song {
        id: id.into(),
        ..Default::default()
    }
}

#[test]
fn reopening_the_same_artist_rejects_both_old_requests() {
    let mut details = ArtistDetails::default();
    let mut requests = LibraryRequests::default();
    details.select(artist("same"));
    let old_albums = details.begin_albums(&mut requests).unwrap();
    let old_top = details.begin_top_songs(&mut requests).unwrap();
    details.select(artist("same"));
    let albums = details.begin_albums(&mut requests).unwrap();
    let top = details.begin_top_songs(&mut requests).unwrap();
    assert!(details
        .complete_albums(old_albums, Ok(vec![album("stale")]))
        .is_none());
    assert!(details
        .complete_albums(old_albums, Err("stale albums error".into()))
        .is_none());
    assert!(details
        .complete_top_songs(old_top, Ok(vec![song("stale")]))
        .is_none());
    assert!(details
        .complete_top_songs(old_top, Err("stale songs error".into()))
        .is_none());
    assert!(details.albums_loading());
    assert!(details.top_songs_loading());
    assert!(details.albums_error().is_none());
    assert!(details.top_songs_error().is_none());
    details.complete_albums(albums, Ok(vec![album("current")]));
    details.complete_top_songs(top, Ok(vec![song("current")]));
    assert_eq!(details.albums()[0].id, "current");
    assert_eq!(details.top_songs()[0].id, "current");
}

#[test]
fn album_and_popular_song_loads_complete_independently_in_either_order() {
    for albums_first in [true, false] {
        let mut details = ArtistDetails::default();
        let mut requests = LibraryRequests::default();
        details.select(artist("A"));
        let albums = details.begin_albums(&mut requests).unwrap();
        let top = details.begin_top_songs(&mut requests).unwrap();
        assert!(details
            .complete_albums(top, Ok(vec![album("wrong request")]))
            .is_none());
        assert!(details
            .complete_top_songs(albums, Ok(vec![song("wrong request")]))
            .is_none());
        for complete_albums in [albums_first, !albums_first] {
            if complete_albums {
                details.complete_albums(albums, Ok(vec![album("A1")]));
                assert!(!details.albums_loading());
                if albums_first {
                    assert!(details.top_songs_loading());
                }
            } else {
                details.complete_top_songs(top, Ok(vec![song("popular")]));
                assert!(!details.top_songs_loading());
                if !albums_first {
                    assert!(details.albums_loading());
                }
            }
        }
        assert_eq!(details.albums()[0].id, "A1");
        assert_eq!(details.top_songs()[0].id, "popular");
    }
}

#[test]
fn top_song_failure_and_retry_preserve_album_loading_and_reject_late_errors() {
    let mut details = ArtistDetails::default();
    let mut requests = LibraryRequests::default();
    details.select(artist("A"));
    let albums = details.begin_albums(&mut requests).unwrap();
    let first_top = details.begin_top_songs(&mut requests).unwrap();
    details.complete_top_songs(first_top, Ok(vec![song("previous")]));
    let failed_top = details.begin_top_songs(&mut requests).unwrap();
    assert_eq!(
        details
            .complete_top_songs(failed_top, Err("offline".into()))
            .unwrap()
            .unwrap_err(),
        "offline"
    );
    assert!(details.albums_loading());
    assert!(details.top_songs().is_empty());
    assert_eq!(details.top_songs_error(), Some("offline"));
    let retry = details.begin_top_songs(&mut requests).unwrap();
    assert!(details.top_songs_error().is_none());
    assert!(details
        .complete_top_songs(failed_top, Err("late error".into()))
        .is_none());
    assert!(details.top_songs_loading());
    details.complete_top_songs(retry, Ok(vec![song("retried")]));
    assert!(details.albums_loading());
    details.complete_albums(albums, Ok(vec![album("A1")]));
    assert_eq!(details.top_songs()[0].id, "retried");
}

#[test]
fn albums_can_be_retried_without_resetting_popular_songs() {
    let mut details = ArtistDetails::default();
    let mut requests = LibraryRequests::default();
    details.select(artist("A"));
    let top = details.begin_top_songs(&mut requests).unwrap();
    details.complete_top_songs(top, Ok(vec![song("popular")]));
    let albums = details.begin_albums(&mut requests).unwrap();
    details.complete_albums(albums, Err("albums unavailable".into()));
    assert_eq!(details.albums_error(), Some("albums unavailable"));
    assert!(details.top_songs_error().is_none());
    let retry = details.begin_albums(&mut requests).unwrap();
    assert!(details.albums_error().is_none());
    assert!(details
        .complete_albums(albums, Ok(vec![album("stale")]))
        .is_none());
    assert_eq!(details.top_songs()[0].id, "popular");
    details.complete_albums(retry, Ok(vec![album("retried")]));
    assert_eq!(details.albums()[0].id, "retried");
}

#[test]
fn changing_artist_clears_data_errors_and_pending_loads() {
    let mut details = ArtistDetails::default();
    let mut requests = LibraryRequests::default();
    assert!(details.begin_albums(&mut requests).is_none());
    assert!(details.begin_top_songs(&mut requests).is_none());
    details.select(artist("A"));
    let albums = details.begin_albums(&mut requests).unwrap();
    details.complete_albums(albums, Ok(vec![album("A1")]));
    let top = details.begin_top_songs(&mut requests).unwrap();
    details.complete_top_songs(top, Err("unavailable".into()));
    let pending = details.begin_albums(&mut requests).unwrap();
    details.select(artist("B"));
    assert_eq!(details.artist().unwrap().id, "B");
    assert!(details.albums().is_empty());
    assert!(details.top_songs().is_empty());
    assert!(details.albums_error().is_none());
    assert!(details.top_songs_error().is_none());
    assert!(!details.albums_loading());
    assert!(!details.top_songs_loading());
    assert!(details
        .complete_albums(pending, Ok(vec![album("A2")]))
        .is_none());
}

#[test]
fn server_reset_rejects_old_results_even_with_the_same_artist_id() {
    let mut state = AppState::default();
    let mut requests = LibraryRequests::default();
    state.artist_details.select(artist("shared-id"));
    let old_albums = state.artist_details.begin_albums(&mut requests).unwrap();
    let old_top = state.artist_details.begin_top_songs(&mut requests).unwrap();
    state = AppState::default();
    state.artist_details.select(artist("shared-id"));
    let albums = state.artist_details.begin_albums(&mut requests).unwrap();
    let top = state.artist_details.begin_top_songs(&mut requests).unwrap();
    assert!(state
        .artist_details
        .complete_albums(old_albums, Ok(vec![album("old-server")]))
        .is_none());
    assert!(state
        .artist_details
        .complete_top_songs(old_top, Err("old-server error".into()))
        .is_none());
    assert!(state.artist_details.albums_loading());
    assert!(state.artist_details.top_songs_loading());
    state
        .artist_details
        .complete_albums(albums, Ok(vec![album("new-server")]));
    state
        .artist_details
        .complete_top_songs(top, Ok(vec![song("new-server")]));
    assert_eq!(state.artist_details.albums()[0].id, "new-server");
    assert_eq!(state.artist_details.top_songs()[0].id, "new-server");
}
