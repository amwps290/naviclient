use super::*;
use crate::app::playlists::search_index::Cancellation;

fn filtered() -> AppState {
    let mut state = AppState::default();
    let mut requests = LibraryRequests::default();
    let playlist = Playlist {
        id: "p".into(),
        owner: Some("alice".into()),
        ..Default::default()
    };
    state.playlists.select(playlist.clone());
    let request = state.playlists.begin_detail(&mut requests).unwrap();
    state.playlists.complete_detail(
        request,
        Ok(PlaylistDetails {
            playlist,
            unavailable_count: 0,
            songs: vec![song("A"), song("B"), song("A"), song("C")],
        }),
    );
    state.playlists.search.set_query("a".into());
    let (request, index, terms) = state.playlists.search.start().unwrap();
    state.playlists.search.complete(
        request,
        Ok(index.matching(&terms, &Cancellation::default()).unwrap()),
    );
    state
}

#[test]
fn filtered_menu_captures_full_order_and_original_duplicate_position() {
    let state = filtered();
    let context = SongMenuContext::for_scope(&state, SongScope::PlaylistDetail, 1).unwrap();
    assert_eq!(context.song.title, "A");
    assert!(context.source.can_play(&state));
    let SongMenuSource::Library {
        songs,
        index,
        playlist,
        playlist_target,
    } = context.source
    else {
        panic!("library menu expected")
    };
    assert_eq!(index, 2, "second matching A is the third original entry");
    assert_eq!(
        songs
            .iter()
            .map(|song| song.title.as_str())
            .collect::<Vec<_>>(),
        ["A", "B", "A", "C"]
    );
    assert!(playlist_target.unwrap().filtered);
    let edit = PlaylistEdit::Remove {
        id: playlist.unwrap().id,
        expected: song_ids(&songs),
        index,
    };
    assert!(edit
        .validate_entries(
            &state
                .playlists
                .songs()
                .iter()
                .map(|song| song.id.clone())
                .collect::<Vec<_>>()
        )
        .is_ok());
    let PlaylistEdit::Remove {
        index, expected, ..
    } = edit
    else {
        unreachable!()
    };
    assert_eq!(index, 2);
    assert_eq!(
        expected.len(),
        4,
        "server order validation must use the full playlist, not matches"
    );
}

#[test]
fn query_edits_invalidate_open_menus_and_pending_results_have_no_menu() {
    let mut state = filtered();
    let context = SongMenuContext::for_scope(&state, SongScope::PlaylistDetail, 1).unwrap();
    state.playlists.search.set_query("b".into());
    assert!(!context.source.can_play(&state));
    assert!(!context.source.playlist_target().unwrap().is_current(&state));
    assert!(SongMenuContext::for_scope(&state, SongScope::PlaylistDetail, 0).is_none());
    let (request, index, terms) = state.playlists.search.start().unwrap();
    state.playlists.search.complete(
        request,
        Ok(index.matching(&terms, &Cancellation::default()).unwrap()),
    );
    assert!(!context.source.can_play(&state));
    assert_eq!(
        SongMenuContext::for_scope(&state, SongScope::PlaylistDetail, 0)
            .unwrap()
            .song
            .title,
        "B"
    );
}
