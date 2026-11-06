use super::*;

fn playlist(id: &str) -> Playlist {
    Playlist {
        id: id.to_string(),
        name: id.to_string(),
        ..Playlist::default()
    }
}

fn song(id: &str, cover: Option<&str>) -> Song {
    Song {
        id: id.to_string(),
        title: id.to_string(),
        cover_art: cover.map(str::to_string),
        ..Song::default()
    }
}

fn details(id: &str) -> PlaylistDetails {
    PlaylistDetails {
        playlist: playlist(id),
        songs: vec![
            song("a", None),
            song("b", Some("b-cover")),
            song("a", Some("a-cover")),
        ],
    }
}

fn edited(id: &str, deleted: bool) -> PlaylistEdited {
    PlaylistEdited {
        id: id.to_string(),
        deleted,
    }
}

fn loaded(requests: &mut LibraryRequests) -> PlaylistLibrary {
    let mut state = PlaylistLibrary::default();
    let list = state.begin_list(requests);
    state.complete_list(list, Ok(vec![playlist("p"), playlist("other")]));
    state.select(playlist("p"));
    let detail = state.begin_detail(requests).unwrap();
    state.complete_detail(detail, Ok(details("p")));
    state
}

#[test]
fn list_and_detail_loads_are_independent_and_ignore_wrong_tickets() {
    let mut requests = LibraryRequests::default();
    let mut state = PlaylistLibrary::default();
    assert!(state.begin_detail(&mut requests).is_none());
    state.select(playlist("p"));
    let list = state.begin_list(&mut requests);
    let detail = state.begin_detail(&mut requests).unwrap();
    assert!(state
        .complete_list(detail, Ok(vec![playlist("wrong")]))
        .is_none());
    assert!(state.complete_detail(list, Ok(details("wrong"))).is_none());
    assert!(state.list_loading());
    assert!(state.detail_loading());
    assert!(state.complete_list(list, Ok(vec![playlist("p")])).is_some());
    assert!(!state.list_loading());
    assert!(state.detail_loading());
    assert!(state.complete_detail(detail, Ok(details("p"))).is_some());
    assert!(!state.detail_loading());
    assert!(!state.editing());
}

#[test]
fn refreshed_list_rejects_old_and_duplicate_results_and_preserves_data_on_failure() {
    let mut requests = LibraryRequests::default();
    let mut state = loaded(&mut requests);
    let old = state.begin_list(&mut requests);
    let current = state.begin_list(&mut requests);
    assert!(state
        .complete_list(old, Ok(vec![playlist("old")]))
        .is_none());
    assert!(state
        .complete_list(old, Err("old failure".into()))
        .is_none());
    assert!(state.list_loading());
    assert!(state
        .complete_list(current, Err("offline".into()))
        .unwrap()
        .is_err());
    assert!(!state.list_loading());
    assert_eq!(state.items()[0].id, "p");
    assert!(state
        .complete_list(current, Ok(vec![playlist("duplicate")]))
        .is_none());
    assert_eq!(state.items()[0].id, "p");
    let retry = state.begin_list(&mut requests);
    state.complete_list(retry, Ok(vec![playlist("new")]));
    assert_eq!(state.items()[0].id, "new");
}

#[test]
fn returning_to_a_playlist_rejects_its_previous_detail_results() {
    let mut requests = LibraryRequests::default();
    let mut state = loaded(&mut requests);
    let old_p = state.begin_detail(&mut requests).unwrap();
    state.select(playlist("other"));
    assert!(state.songs().is_empty());
    assert!(!state.detail_loading());
    let other = state.begin_detail(&mut requests).unwrap();
    state.select(playlist("p"));
    let current_p = state.begin_detail(&mut requests).unwrap();
    assert!(state.complete_detail(old_p, Ok(details("p"))).is_none());
    assert!(state
        .complete_detail(old_p, Err("old failure".into()))
        .is_none());
    assert!(state.complete_detail(other, Ok(details("other"))).is_none());
    assert!(state.detail_loading());
    assert_eq!(state.playlist().unwrap().id, "p");
    assert!(state.songs().is_empty());
    state.complete_detail(current_p, Ok(details("p")));
    assert_eq!(state.songs().len(), 3);
}

#[test]
fn failed_detail_refresh_preserves_metadata_and_duplicate_occurrences() {
    let mut requests = LibraryRequests::default();
    let mut state = loaded(&mut requests);
    let old = state.begin_detail(&mut requests).unwrap();
    let current = state.begin_detail(&mut requests).unwrap();
    assert!(state.complete_detail(old, Err("stale".into())).is_none());
    assert!(state
        .complete_detail(current, Err("offline".into()))
        .unwrap()
        .is_err());
    assert!(!state.detail_loading());
    assert_eq!(state.playlist().unwrap().id, "p");
    assert_eq!(crate::playlist::song_ids(state.songs()), ["a", "b", "a"]);
    assert!(state
        .complete_detail(current, Ok(details("duplicate")))
        .is_none());
    assert_eq!(state.playlist().unwrap().id, "p");
    let retry = state.begin_detail(&mut requests).unwrap();
    let mut refreshed = details("p");
    refreshed.songs.remove(1);
    state.complete_detail(retry, Ok(refreshed));
    assert_eq!(crate::playlist::song_ids(state.songs()), ["a", "a"]);
}

#[test]
fn artwork_falls_back_to_first_available_song_without_overriding_server_artwork() {
    let mut requests = LibraryRequests::default();
    let mut state = loaded(&mut requests);
    assert_eq!(
        state.playlist().unwrap().cover_art.as_deref(),
        Some("b-cover")
    );
    assert_eq!(state.items()[0].cover_art.as_deref(), Some("b-cover"));
    assert!(state.items()[1].cover_art.is_none());
    let request = state.begin_detail(&mut requests).unwrap();
    let mut response = details("p");
    response.playlist.cover_art = Some("server-cover".into());
    state.complete_detail(request, Ok(response));
    assert_eq!(
        state.playlist().unwrap().cover_art.as_deref(),
        Some("server-cover")
    );
    assert_eq!(state.items()[0].cover_art.as_deref(), Some("server-cover"));
    assert_eq!(crate::playlist::song_ids(state.songs()), ["a", "b", "a"]);
}

#[test]
fn starting_an_edit_invalidates_reads_and_clears_their_loading_flags() {
    let mut requests = LibraryRequests::default();
    let mut state = loaded(&mut requests);
    let list = state.begin_list(&mut requests);
    let detail = state.begin_detail(&mut requests).unwrap();
    let edit = state
        .begin_edit(&mut requests, PlaylistEditOrigin::Menu)
        .unwrap();
    assert!(state.editing());
    assert!(!state.list_loading());
    assert!(!state.detail_loading());
    assert!(state
        .begin_edit(&mut requests, PlaylistEditOrigin::Menu)
        .is_none());
    assert!(state
        .complete_list(list, Ok(vec![playlist("stale")]))
        .is_none());
    assert!(state
        .complete_detail(detail, Err("stale failure".into()))
        .is_none());
    assert_eq!(state.items()[0].id, "p");
    assert_eq!(state.songs().len(), 3);
    assert!(state.complete_edit(edit, Ok(edited("p", false))).is_some());
    assert!(!state.editing());
}

#[test]
fn finishing_an_edit_invalidates_reads_started_while_it_was_pending() {
    for succeeded in [true, false] {
        let mut requests = LibraryRequests::default();
        let mut state = loaded(&mut requests);
        let edit = state
            .begin_edit(&mut requests, PlaylistEditOrigin::Menu)
            .unwrap();
        let list = state.begin_list(&mut requests);
        let detail = state.begin_detail(&mut requests).unwrap();
        let result = if succeeded {
            Ok(edited("p", false))
        } else {
            Err("offline".into())
        };
        assert!(state.complete_edit(edit, result).is_some());
        assert!(!state.editing());
        assert!(!state.list_loading());
        assert!(!state.detail_loading());
        assert!(state
            .complete_list(list, Ok(vec![playlist("before write")]))
            .is_none());
        assert!(state
            .complete_detail(detail, Ok(details("before write")))
            .is_none());
        assert!(state
            .complete_list(list, Err("old failure".into()))
            .is_none());
        assert!(state
            .complete_detail(detail, Err("old failure".into()))
            .is_none());
        assert_eq!(state.items()[0].id, "p");
        assert_eq!(state.playlist().unwrap().id, "p");
    }
}

#[test]
fn duplicate_edit_results_cannot_invalidate_new_reads_or_unlock_a_new_edit() {
    let mut requests = LibraryRequests::default();
    let mut state = loaded(&mut requests);
    let old_edit = state
        .begin_edit(&mut requests, PlaylistEditOrigin::Menu)
        .unwrap();
    state.complete_edit(old_edit, Ok(edited("p", false)));
    let list = state.begin_list(&mut requests);
    let detail = state.begin_detail(&mut requests).unwrap();
    assert!(state
        .complete_edit(old_edit, Ok(edited("p", true)))
        .is_none());
    assert!(state
        .complete_edit(old_edit, Err("duplicate failure".into()))
        .is_none());
    assert!(state.list_loading());
    assert!(state.detail_loading());
    assert!(state.complete_list(list, Ok(vec![playlist("p")])).is_some());
    assert!(state.complete_detail(detail, Ok(details("p"))).is_some());
    let new_edit = state
        .begin_edit(&mut requests, PlaylistEditOrigin::Menu)
        .unwrap();
    assert!(state
        .complete_edit(old_edit, Err("duplicate failure".into()))
        .is_none());
    assert!(state.editing());
    assert!(state
        .begin_edit(&mut requests, PlaylistEditOrigin::Menu)
        .is_none());
    state.complete_edit(new_edit, Ok(edited("p", false)));
    assert!(!state.editing());
}

#[test]
fn deleting_selected_playlist_removes_list_item_and_detail_together() {
    let mut requests = LibraryRequests::default();
    let mut state = loaded(&mut requests);
    let edit = state
        .begin_edit(&mut requests, PlaylistEditOrigin::Menu)
        .unwrap();
    state.complete_edit(edit, Ok(edited("p", true)));
    assert_eq!(state.items().len(), 1);
    assert_eq!(state.items()[0].id, "other");
    assert_eq!(state.visible_items().len(), 1);
    assert!(state.playlist().is_none());
    assert!(state.songs().is_empty());
    assert!(state.begin_detail(&mut requests).is_none());
}

#[test]
fn deleting_another_playlist_keeps_the_selected_detail_and_song_order() {
    let mut requests = LibraryRequests::default();
    let mut state = loaded(&mut requests);
    let edit = state
        .begin_edit(&mut requests, PlaylistEditOrigin::Menu)
        .unwrap();
    state.complete_edit(edit, Ok(edited("other", true)));
    assert_eq!(state.items().len(), 1);
    assert_eq!(state.playlist().unwrap().id, "p");
    assert_eq!(crate::playlist::song_ids(state.songs()), ["a", "b", "a"]);
}

#[test]
fn failed_edits_keep_data_and_allow_retry() {
    let mut requests = LibraryRequests::default();
    let mut state = loaded(&mut requests);
    let edit = state
        .begin_edit(&mut requests, PlaylistEditOrigin::Menu)
        .unwrap();
    assert!(state
        .complete_edit(edit, Err("denied".into()))
        .unwrap()
        .result
        .is_err());
    assert!(!state.editing());
    assert_eq!(state.items().len(), 2);
    assert_eq!(state.playlist().unwrap().id, "p");
    assert_eq!(state.songs().len(), 3);
    let retry = state
        .begin_edit(&mut requests, PlaylistEditOrigin::Menu)
        .unwrap();
    assert_ne!(edit, retry);
    assert!(state.complete_edit(edit, Ok(edited("p", true))).is_none());
    assert!(state.editing());
    state.complete_edit(retry, Ok(edited("p", true)));
    assert!(state.playlist().is_none());
}

#[test]
fn reset_rejects_old_server_reads_and_edits_with_reused_playlist_ids() {
    let mut requests = LibraryRequests::default();
    let mut app = crate::app::AppState::default();
    app.playlists.select(playlist("p"));
    let old_edit = app
        .playlists
        .begin_edit(&mut requests, PlaylistEditOrigin::Import)
        .unwrap();
    let old_list = app.playlists.begin_list(&mut requests);
    let old_detail = app.playlists.begin_detail(&mut requests).unwrap();
    app = crate::app::AppState::default();
    app.playlists.select(playlist("p"));
    let current_edit = app
        .playlists
        .begin_edit(&mut requests, PlaylistEditOrigin::Dialog)
        .unwrap();
    let current_list = app.playlists.begin_list(&mut requests);
    let current_detail = app.playlists.begin_detail(&mut requests).unwrap();
    for result in [Ok(edited("p", true)), Err("old edit failure".into())] {
        assert!(app.playlists.complete_edit(old_edit, result).is_none());
    }
    for result in [
        Ok(vec![playlist("old server")]),
        Err("old list failure".into()),
    ] {
        assert!(app.playlists.complete_list(old_list, result).is_none());
    }
    for result in [Ok(details("p")), Err("old detail failure".into())] {
        assert!(app.playlists.complete_detail(old_detail, result).is_none());
    }
    assert!(app.playlists.editing());
    assert!(app.playlists.list_loading());
    assert!(app.playlists.detail_loading());
    assert!(app.playlists.items().is_empty());
    assert!(app.playlists.songs().is_empty());
    assert_eq!(app.playlists.playlist().unwrap().id, "p");
    app.playlists
        .complete_list(current_list, Ok(vec![playlist("new server")]));
    app.playlists
        .complete_detail(current_detail, Ok(details("p")));
    assert_eq!(app.playlists.items()[0].id, "new server");
    assert_eq!(app.playlists.songs().len(), 3);
    let completion = app
        .playlists
        .complete_edit(current_edit, Ok(edited("p", false)))
        .unwrap();
    assert_eq!(completion.origin, PlaylistEditOrigin::Dialog);
    assert_eq!(completion.result.unwrap().id, "p");
    assert!(!app.playlists.editing());
}

#[test]
fn visible_items_remain_bounded_across_expansion_refresh_and_failure() {
    let mut requests = LibraryRequests::default();
    let mut state = PlaylistLibrary::default();
    state.show_more();
    assert!(state.visible_items().is_empty());
    let request = state.begin_list(&mut requests);
    state.complete_list(
        request,
        Ok((0..220).map(|i| playlist(&i.to_string())).collect()),
    );
    assert_eq!(state.visible_items().len(), 50);
    state.show_more();
    assert_eq!(state.visible_items().len(), 150);
    state.show_more();
    assert_eq!(state.visible_items().len(), 220);
    state.show_more();
    assert_eq!(state.visible_items().len(), 220);
    let failed = state.begin_list(&mut requests);
    state.complete_list(failed, Err("offline".into()));
    assert_eq!(state.visible_items().len(), 220);
    let refreshed = state.begin_list(&mut requests);
    state.complete_list(refreshed, Ok(vec![playlist("p")]));
    assert_eq!(state.visible_items().len(), 1);
    let empty = state.begin_list(&mut requests);
    state.complete_list(empty, Ok(Vec::new()));
    assert!(state.visible_items().is_empty());
}
