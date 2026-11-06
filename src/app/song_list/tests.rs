use super::*;
use crate::app::song_playback::SongPlayback;
use crate::models::{PlaybackQueue, QueueOrigin};

fn song(title: &str) -> Song {
    Song {
        id: "duplicate".into(),
        title: title.into(),
        ..Default::default()
    }
}

#[test]
fn song_projections_borrow_the_original_data_and_keep_occurrence_order() {
    let songs = vec![song("First"), song("Second")];
    let queue = PlaybackQueue::from_snapshot(
        songs.clone(),
        vec![QueueOrigin::Manual, QueueOrigin::DailyMix],
    );
    for list in [SongList::Songs(&songs), SongList::Queue(queue.entries())] {
        assert_eq!(list.len(), 2);
        assert!(!list.is_empty());
        assert_eq!(
            list.iter()
                .map(|song| song.title.as_str())
                .collect::<Vec<_>>(),
            ["First", "Second"]
        );
        assert_eq!(
            list.iter()
                .rev()
                .map(|song| song.title.as_str())
                .collect::<Vec<_>>(),
            ["Second", "First"]
        );
        assert_eq!(list.last().unwrap().title, "Second");
        assert!(list.get(2).is_none());
        assert!(list.get(usize::MAX).is_none());
        assert_eq!(
            list.to_vec()
                .iter()
                .map(|song| song.title.as_str())
                .collect::<Vec<_>>(),
            ["First", "Second"]
        );
    }
    assert!(std::ptr::eq(
        SongList::Songs(&songs).get(1).unwrap(),
        &songs[1]
    ));
    assert!(std::ptr::eq(
        SongList::Queue(queue.entries()).get(1).unwrap(),
        queue.song(1).unwrap()
    ));
}

#[test]
fn empty_projections_have_no_rows_or_last_song() {
    for list in [SongList::Songs(&[]), SongList::Queue(&[])] {
        assert!(list.is_empty());
        assert!(list.last().is_none());
        assert!(list.get(0).is_none());
        assert_eq!(list.iter().count(), 0);
    }
}

#[test]
fn scoped_lists_read_the_current_state_including_queue_and_history() {
    let mut state = AppState {
        queue: PlaybackQueue::from_songs(vec![song("Queue")], QueueOrigin::DailyMix),
        ..Default::default()
    };
    state.album_details.select(Album::default());
    let mut requests = super::super::library_requests::LibraryRequests::default();
    let request = state.search.begin(&mut requests);
    state.search.complete(
        request,
        Ok(SearchResults {
            songs: vec![song("Search")],
            ..Default::default()
        }),
    );
    state.playlists.select(Playlist::default());
    let playlist_request = state.playlists.begin_detail(&mut requests).unwrap();
    state.playlists.complete_detail(
        playlist_request,
        Ok(crate::playlist::PlaylistDetails {
            playlist: Playlist::default(),
            songs: vec![song("Playlist")],
        }),
    );
    let request = state.album_details.begin(&mut requests).unwrap();
    state
        .album_details
        .complete(request, Ok(vec![song("Album")]));
    state.artist_details.select(Artist {
        id: "artist".into(),
        name: "Artist".into(),
        album_count: None,
        cover_art: None,
    });
    let request = state.artist_details.begin_top_songs(&mut requests).unwrap();
    state
        .artist_details
        .complete_top_songs(request, Ok(vec![song("Artist")]));
    let request = state.favorites.begin_load(&mut requests);
    state.favorites.complete_load(
        request,
        Ok(Favorites {
            songs: vec![song("Favorite")],
            ..Default::default()
        }),
    );
    let mut history = crate::models::ListeningHistory::default();
    history.record_started(song("History"), 1);
    state.listening.restore(None, history);
    for (scope, title) in [
        (SongScope::AlbumDetail, "Album"),
        (SongScope::ArtistTop, "Artist"),
        (SongScope::PlaylistDetail, "Playlist"),
        (SongScope::Favorites, "Favorite"),
        (SongScope::Search, "Search"),
        (SongScope::Queue, "Queue"),
        (SongScope::History, "History"),
    ] {
        let list = state.songs_for_scope(scope).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list.get(0).unwrap().title, title);
    }
    assert!(std::ptr::eq(
        state
            .songs_for_scope(SongScope::Queue)
            .unwrap()
            .get(0)
            .unwrap(),
        state.queue.song(0).unwrap()
    ));
    assert!(std::ptr::eq(
        state
            .songs_for_scope(SongScope::AlbumDetail)
            .unwrap()
            .get(0)
            .unwrap(),
        &state.album_details.songs()[0]
    ));
    state.queue.clear();
    assert!(state.songs_for_scope(SongScope::Queue).unwrap().is_empty());
    state.search.clear();
    assert!(state.songs_for_scope(SongScope::Search).is_none());
}

#[test]
fn queue_multi_playback_targets_the_first_selected_occurrence_and_keeps_the_complete_queue() {
    let mut state = AppState {
        queue: PlaybackQueue::from_snapshot(
            vec![song("First"), song("Second"), song("Third")],
            vec![
                QueueOrigin::Manual,
                QueueOrigin::DailyMix,
                QueueOrigin::SongRadio,
            ],
        ),
        ..Default::default()
    };
    let entries: Vec<_> = state.queue.entries().iter().map(QueueEntry::id).collect();
    state.song_selection.toggle(SongScope::Queue, 2, 3);
    state.song_selection.toggle(SongScope::Queue, 1, 3);
    assert!(matches!(
        state.selected_playback(SongScope::Queue),
        Some(SongPlayback::QueueIndex(1))
    ));
    assert_eq!(
        state
            .queue
            .entries()
            .iter()
            .map(QueueEntry::id)
            .collect::<Vec<_>>(),
        entries
    );
    assert_eq!(state.queue.len(), 3);
    assert_eq!(state.queue.origin(1), Some(QueueOrigin::DailyMix));
    state.song_selection.clear_multi();
    assert!(state.selected_playback(SongScope::Queue).is_none());
    assert_eq!(state.song_selection.cursor(), Some((SongScope::Queue, 1)));
}

#[test]
fn ordinary_multi_playback_projects_selected_songs_in_row_order_including_duplicates() {
    let mut state = AppState::default();
    let mut requests = LibraryRequests::default();
    let request = state.search.begin(&mut requests);
    state.complete_search(
        request,
        Ok(SearchResults {
            songs: vec![song("First"), song("Second"), song("Third")],
            ..Default::default()
        }),
    );
    state.song_selection.toggle(SongScope::Search, 2, 3);
    state.song_selection.toggle(SongScope::Search, 0, 3);
    let Some(SongPlayback::Replace {
        songs,
        index,
        origin,
    }) = state.selected_playback(SongScope::Search)
    else {
        panic!("expected song projection")
    };
    assert_eq!(index, 0);
    assert_eq!(origin, QueueOrigin::Manual);
    assert_eq!(
        songs
            .iter()
            .map(|song| song.title.as_str())
            .collect::<Vec<_>>(),
        ["First", "Third"]
    );
    assert_eq!(
        songs
            .iter()
            .map(|song| song.id.as_str())
            .collect::<Vec<_>>(),
        ["duplicate", "duplicate"]
    );
    assert!(state.selected_playback(SongScope::Queue).is_none());
    assert!(state.selected_playback(SongScope::Favorites).is_none());
}

#[test]
fn selected_song_iteration_borrows_original_occurrences_in_row_order() {
    let mut state = AppState {
        queue: PlaybackQueue::from_songs(
            vec![song("First"), song("Second"), song("Third")],
            QueueOrigin::Playlist,
        ),
        ..Default::default()
    };
    state.song_selection.toggle(SongScope::Queue, 2, 3);
    state.song_selection.toggle(SongScope::Queue, 0, 3);
    let selected: Vec<_> = state.selected_song_iter(SongScope::Queue).collect();
    assert_eq!(selected.len(), 2);
    assert!(std::ptr::eq(selected[0], state.queue.song(0).unwrap()));
    assert!(std::ptr::eq(selected[1], state.queue.song(2).unwrap()));
    assert_eq!(
        state.selected_song_ids(SongScope::Queue),
        ["duplicate", "duplicate"]
    );
    assert_eq!(state.selected_song_iter(SongScope::Search).count(), 0);
    state.queue.truncate(1);
    assert_eq!(state.selected_song_iter(SongScope::Queue).count(), 1);
    assert_eq!(state.selected_song_ids(SongScope::Queue), ["duplicate"]);
}

#[test]
fn selection_summary_counts_occurrences_and_uses_current_favorite_state() {
    let mut state = AppState {
        queue: PlaybackQueue::from_songs(
            vec![
                song("First"),
                Song {
                    id: "other".into(),
                    ..song("Other")
                },
                song("Third"),
            ],
            QueueOrigin::Playlist,
        ),
        ..Default::default()
    };
    let mut requests = LibraryRequests::default();
    let request = state.favorites.begin_load(&mut requests);
    state.favorites.complete_load(
        request,
        Ok(Favorites {
            songs: vec![song("Favorite")],
            ..Default::default()
        }),
    );
    assert_eq!(
        state.selection_summary(SongScope::Queue),
        SongSelectionSummary::default()
    );
    state.song_selection.toggle(SongScope::Queue, 2, 3);
    state.song_selection.toggle(SongScope::Queue, 0, 3);
    assert_eq!(
        state.selection_summary(SongScope::Queue),
        SongSelectionSummary {
            count: 2,
            all_favorited: true
        },
    );
    state.song_selection.toggle(SongScope::Queue, 1, 3);
    assert_eq!(
        state.selection_summary(SongScope::Queue),
        SongSelectionSummary {
            count: 3,
            all_favorited: false
        },
    );
    let request = state.favorites.begin_load(&mut requests);
    state
        .favorites
        .complete_load(request, Ok(Favorites::default()));
    assert_eq!(state.selection_summary(SongScope::Queue).count, 3);
    assert!(!state.selection_summary(SongScope::Queue).all_favorited);
    assert_eq!(
        state.selection_summary(SongScope::Search),
        SongSelectionSummary::default()
    );
    state.queue.clear();
    assert_eq!(
        state.selection_summary(SongScope::Queue),
        SongSelectionSummary::default()
    );
}
