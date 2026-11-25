use super::*;
use crate::app::songs::playback::SongPlayback;
use crate::app::songs::row_actions::SongRowTarget;
use crate::playlist::PlaylistDetails;

fn load(state: &mut AppState, requests: &mut LibraryRequests) -> LibraryRequest {
    let playlist = Playlist {
        id: "p".into(),
        ..Default::default()
    };
    state.playlists.select(playlist.clone());
    let request = state.playlists.begin_detail(requests).unwrap();
    state.playlists.complete_detail(
        request,
        Ok(PlaylistDetails {
            playlist,
            unavailable_count: 0,
            songs: ["A", "B", "A", "C"]
                .map(|title| Song {
                    id: title.into(),
                    title: title.into(),
                    ..Default::default()
                })
                .to_vec(),
        }),
    );
    request
}

fn filter(state: &mut AppState, query: &str) {
    state.playlists.search.set_query(query.into());
    let (request, index, terms) = state.playlists.search.start().unwrap();
    state.playlists.search.complete(
        request,
        Ok(index.matching(&terms, &Cancellation::default()).unwrap()),
    );
}

#[test]
fn filtered_single_playback_uses_full_playlist_while_multiselect_uses_results() {
    let mut state = AppState::default();
    load(&mut state, &mut LibraryRequests::default());
    filter(&mut state, "a");
    let songs = state.songs_for_scope(SongScope::PlaylistDetail).unwrap();
    assert_eq!(songs.len(), 2);
    assert_eq!(songs.original_index(1), Some(2));
    assert!(std::ptr::eq(
        songs.get(1).unwrap(),
        &state.playlists.songs()[2]
    ));
    let SongPlayback::Replace {
        songs,
        index,
        origin,
    } = state
        .song_playback_at(SongScope::PlaylistDetail, 1)
        .unwrap()
    else {
        panic!("replacement expected")
    };
    assert_eq!(crate::playlist::song_ids(&songs), ["A", "B", "A", "C"]);
    assert_eq!(index, 2);
    assert_eq!(origin, QueueOrigin::Playlist);
    state
        .song_selection
        .select_all(SongScope::PlaylistDetail, 2);
    let SongPlayback::Replace { songs, index, .. } =
        state.selected_playback(SongScope::PlaylistDetail).unwrap()
    else {
        panic!("replacement expected")
    };
    assert_eq!(crate::playlist::song_ids(&songs), ["A", "A"]);
    assert_eq!(index, 0);
    assert_eq!(
        state.selected_song_ids(SongScope::PlaylistDetail),
        ["A", "A"]
    );
}

#[test]
fn changed_queries_and_reopened_playlists_invalidate_old_rows() {
    let mut state = AppState::default();
    let mut requests = LibraryRequests::default();
    load(&mut state, &mut requests);
    filter(&mut state, "a");
    let target = SongRowTarget::capture(&state, SongScope::PlaylistDetail, 1).unwrap();
    assert_eq!(target.resolve(&state), Some((SongScope::PlaylistDetail, 1)));
    state.playlists.search.set_query("b".into());
    assert!(target.resolve(&state).is_none());
    assert!(state
        .song_playback_at(SongScope::PlaylistDetail, 0)
        .is_none());
    filter(&mut state, "a");
    assert!(
        target.resolve(&state).is_none(),
        "A → B → A must not reuse old row targets"
    );
    let current = SongRowTarget::capture(&state, SongScope::PlaylistDetail, 1).unwrap();
    load(&mut state, &mut requests);
    assert!(current.resolve(&state).is_none());
    assert!(state.playlists.search.query.is_empty());
}

#[test]
fn query_changes_clear_only_playlist_selection_and_keep_the_playback_queue() {
    let mut state = AppState::default();
    load(&mut state, &mut LibraryRequests::default());
    state.queue =
        PlaybackQueue::from_songs(state.playlists.songs().to_vec(), QueueOrigin::Playlist);
    state.song_selection.select_all(SongScope::Queue, 4);
    let queue_selection = state.song_selection.clone();
    assert!(state.set_playlist_query("a".into()));
    assert_eq!(state.song_selection, queue_selection);
    assert_eq!(state.queue.song_ids(), ["A", "B", "A", "C"]);
    filter(&mut state, "a");
    state
        .song_selection
        .select_all(SongScope::PlaylistDetail, 2);
    assert!(!state.set_playlist_query(" Ａ a ".into()));
    assert_eq!(
        state.selected_song_ids(SongScope::PlaylistDetail),
        ["A", "A"]
    );
    assert!(state.set_playlist_query("b".into()));
    assert!(state
        .selected_song_ids(SongScope::PlaylistDetail)
        .is_empty());
    assert!(state.song_selection.cursor().is_none());
    assert_eq!(state.queue.song_ids(), ["A", "B", "A", "C"]);
}

#[tokio::test]
async fn streamed_playlist_index_is_published_before_completion_and_filtering_is_local() {
    use crate::test_support::http::Server;
    let tracks = (0..300).map(|i| serde_json::json!({"id": i.to_string(), "title": format!("Track {i}"), "artist": "Artist"})).collect::<Vec<_>>();
    let body = serde_json::json!({"subsonic-response": {"status": "ok", "version": "1.16.1", "playlist": {"id": "p", "name": "Large", "entry": tracks}}}).to_string();
    let server = Server::new(vec![(200, body)]);
    let api = MusicSource::opensubsonic("test", &server.url(""), "u", "p").unwrap();
    let request = LibraryRequests::default().next();
    let (updates, mut rx) = playlist_update_channel();
    let (tx, messages) = mpsc::channel();
    let task = tokio::spawn(load_indexed_playlist(api, "p".into(), request, updates, tx));
    let mut count = 0;
    loop {
        let update = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        match update {
            PlaylistUpdate::Batch { songs, .. } => {
                assert!(songs.len() <= 128);
                count += songs.len();
            }
            PlaylistUpdate::Complete(_) => break,
            PlaylistUpdate::Failed(error) => panic!("{}", error.message),
            _ => {}
        }
    }
    assert_eq!(count, 300);
    let Msg::PlaylistIndexed {
        request: result_request,
        index,
    } = messages.try_recv().unwrap()
    else {
        panic!("index expected before Complete")
    };
    assert_eq!(request, result_request);
    assert_eq!(index.len(), 300);
    for query in ["Track 299", "Artist Track 1", "missing", ""] {
        assert!(index
            .matching(
                &super::super::search_index::QueryTerms::new(query),
                &Cancellation::default()
            )
            .is_some());
    }
    assert_eq!(
        server.requests().len(),
        1,
        "local search never issues search3 or reloads the playlist"
    );
    task.await.unwrap();
}

#[tokio::test]
async fn empty_playlist_publishes_an_empty_index_and_loading_failure_does_not() {
    use crate::test_support::http::Server;
    for (status, body, success) in [
        (200, serde_json::json!({"subsonic-response": {"status": "ok", "playlist": {"id": "p", "name": "Empty", "entry": []}}}).to_string(), true),
        (503, "unavailable".into(), false),
    ] {
        let server = Server::new(vec![(status, body)]);
        let api = MusicSource::opensubsonic("test", &server.url(""), "u", "p").unwrap();
        let request = LibraryRequests::default().next();
        let (updates, mut rx) = playlist_update_channel();
        let (tx, messages) = mpsc::channel();
        let task = tokio::spawn(load_indexed_playlist(api, "p".into(), request, updates, tx));
        let mut completed = false;
        while let Some(update) = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await.unwrap() {
            if matches!(update, PlaylistUpdate::Complete(_)) { completed = true; }
            if matches!(update, PlaylistUpdate::Complete(_) | PlaylistUpdate::Failed(_)) { break; }
        }
        assert_eq!(completed, success);
        if success {
            let Msg::PlaylistIndexed { index, .. } = messages.try_recv().unwrap() else { panic!("index expected") };
            assert_eq!(index.len(), 0);
        } else { assert!(messages.try_recv().is_err()); }
        task.await.unwrap();
    }
}
