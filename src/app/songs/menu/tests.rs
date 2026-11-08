use super::*;
use crate::playlist::PlaylistDetails;

fn song(title: &str) -> Song {
    Song {
        id: "duplicate".into(),
        title: title.into(),
        ..Default::default()
    }
}

fn queue_state() -> AppState {
    AppState {
        queue: PlaybackQueue::from_snapshot(
            vec![song("First"), song("Second"), song("Third")],
            vec![
                QueueOrigin::Manual,
                QueueOrigin::DailyMix,
                QueueOrigin::Playlist,
            ],
        ),
        ..Default::default()
    }
}

#[test]
fn queue_menu_tracks_the_selected_occurrence_through_moves_and_other_removals() {
    for selected in 0..3 {
        for from in 0..3 {
            for to in 0..3 {
                let mut state = queue_state();
                let context =
                    SongMenuContext::for_scope(&state, SongScope::Queue, selected).unwrap();
                let SongMenuSource::Queue(entry) = context.source else {
                    panic!("queue menus must hold an occurrence identity");
                };
                assert_eq!(state.queue.entry_id(selected), Some(entry));
                state.queue.move_entry(from, to);
                let index = queue_menu_index(&state.queue, entry).unwrap();
                assert_eq!(state.queue.song(index).unwrap().title, context.song.title);
                let other = (index + 1) % state.queue.len();
                state.queue.remove(other);
                let index = queue_menu_index(&state.queue, entry).unwrap();
                assert_eq!(state.queue.song(index).unwrap().title, context.song.title);
                assert_eq!(state.queue.entry_id(index), Some(entry));
            }
        }
    }
}

#[test]
fn removing_the_selected_occurrence_never_redirects_to_another_copy_of_the_song() {
    for selected in 0..3 {
        let mut state = queue_state();
        let context = SongMenuContext::for_scope(&state, SongScope::Queue, selected).unwrap();
        let SongMenuSource::Queue(entry) = context.source else {
            panic!("expected a queue occurrence");
        };
        state.queue.remove(selected);
        assert_eq!(state.queue.song_ids(), ["duplicate", "duplicate"]);
        assert_eq!(queue_menu_index(&state.queue, entry), None);
        state
            .queue
            .insert(selected, vec![context.song], QueueOrigin::Manual);
        assert_eq!(
            state.queue.song_ids(),
            ["duplicate", "duplicate", "duplicate"]
        );
        assert_eq!(queue_menu_index(&state.queue, entry), None);
    }
}

#[test]
fn queue_replacement_or_session_restoration_invalidates_an_old_menu() {
    let mut state = queue_state();
    let context = SongMenuContext::for_scope(&state, SongScope::Queue, 1).unwrap();
    let SongMenuSource::Queue(entry) = context.source else {
        panic!("expected a queue occurrence");
    };
    let (songs, origins) = state.queue.snapshot();
    let expected = state.queue.song_ids();
    state.queue = PlaybackQueue::from_snapshot(songs.clone(), origins);
    assert_eq!(state.queue.song_ids(), expected);
    assert_eq!(queue_menu_index(&state.queue, entry), None);
    state.queue = PlaybackQueue::from_songs(songs, QueueOrigin::Manual);
    assert_eq!(state.queue.song_ids(), expected);
    assert_eq!(queue_menu_index(&state.queue, entry), None);
    state.queue.clear();
    assert_eq!(queue_menu_index(&state.queue, entry), None);
}

#[test]
fn playlist_menu_retains_its_original_order_and_owner_after_browsing_elsewhere() {
    let mut state = AppState::default();
    let mut requests = LibraryRequests::default();
    let playlist = Playlist {
        id: "original".into(),
        name: "Original playlist".into(),
        owner: Some("alice".into()),
        ..Default::default()
    };
    state.playlists.select(playlist.clone());
    let request = state.playlists.begin_detail(&mut requests).unwrap();
    state.playlists.complete_detail(
        request,
        Ok(PlaylistDetails {
            unavailable_count: 0,
            playlist,
            songs: vec![song("First"), song("Second"), song("Third")],
        }),
    );
    let context = SongMenuContext::for_scope(&state, SongScope::PlaylistDetail, 1).unwrap();
    state.playlists.select(Playlist {
        id: "other".into(),
        ..Default::default()
    });
    assert_eq!(context.song.title, "Second");
    let SongMenuSource::Library {
        songs,
        index,
        playlist,
    } = context.source
    else {
        panic!("library menus must hold their original list snapshot");
    };
    assert_eq!(index, 1);
    assert_eq!(song_ids(&songs), ["duplicate", "duplicate", "duplicate"]);
    assert_eq!(
        songs
            .iter()
            .map(|song| song.title.as_str())
            .collect::<Vec<_>>(),
        ["First", "Second", "Third"],
    );
    let playlist = playlist.unwrap();
    assert_eq!(playlist.id, "original");
    assert!(can_edit(&playlist, "alice"));
    assert!(!can_edit(&playlist, "bob"));
}

#[test]
fn library_menu_snapshots_have_no_queue_edit_identity() {
    let mut state = AppState::default();
    let mut requests = LibraryRequests::default();
    let request = state.search.begin(&mut requests);
    state.search.complete(
        request,
        Ok(SearchResults {
            songs: vec![song("First"), song("Second")],
            ..Default::default()
        }),
    );
    let context = SongMenuContext::for_scope(&state, SongScope::Search, 1).unwrap();
    let SongMenuSource::Library {
        songs,
        index,
        playlist,
    } = context.source
    else {
        panic!("search rows must retain their library list");
    };
    assert_eq!(index, 1);
    assert!(playlist.is_none());
    assert_eq!(songs[index].title, "Second");
}

#[test]
fn missing_lists_and_out_of_bounds_rows_cannot_create_menu_actions() {
    let state = queue_state();
    for (scope, index) in [
        (SongScope::ArtistTop, 0),
        (SongScope::Queue, state.queue.len()),
        (SongScope::Queue, usize::MAX),
        (SongScope::Search, 0),
        (SongScope::PlaylistDetail, 0),
    ] {
        assert!(SongMenuContext::for_scope(&state, scope, index).is_none());
    }
}
