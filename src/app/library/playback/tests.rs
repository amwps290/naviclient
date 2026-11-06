use super::*;

fn song(id: &str, track: i32) -> Song {
    Song {
        id: id.into(),
        track: Some(track),
        ..Default::default()
    }
}

#[test]
fn newer_album_or_artist_intent_rejects_previous_results() {
    for (old_source, new_source) in [
        (PlaybackSource::Album, PlaybackSource::Album),
        (PlaybackSource::Album, PlaybackSource::Artist),
        (PlaybackSource::Artist, PlaybackSource::Album),
        (PlaybackSource::Artist, PlaybackSource::Artist),
    ] {
        let mut playback = LibraryPlayback::default();
        let mut requests = LibraryRequests::default();
        let old = playback.begin(&mut requests, old_source);
        let current = playback.begin(&mut requests, new_source);
        assert!(playback.complete(old, Ok(vec![song("stale", 1)])).is_none());
        assert!(playback.complete(old, Err("stale error".into())).is_none());
        let songs = playback
            .complete(current, Ok(vec![song("current", 1)]))
            .unwrap()
            .unwrap();
        assert_eq!(songs[0].id, "current");
        assert!(playback
            .complete(current, Ok(vec![song("duplicate", 1)]))
            .is_none());
    }
}

#[test]
fn cancellation_rejects_late_success_and_failure_without_consuming_a_new_intent() {
    let mut playback = LibraryPlayback::default();
    let mut requests = LibraryRequests::default();
    let cancelled = playback.begin(&mut requests, PlaybackSource::Album);
    playback.cancel();
    assert!(playback
        .complete(cancelled, Ok(vec![song("cancelled", 1)]))
        .is_none());
    assert!(playback
        .complete(cancelled, Err("cancelled error".into()))
        .is_none());
    let current = playback.begin(&mut requests, PlaybackSource::Artist);
    assert!(playback
        .complete(cancelled, Ok(vec![song("cancelled", 1)]))
        .is_none());
    assert!(playback
        .complete(current, Ok(vec![song("current", 1)]))
        .unwrap()
        .is_ok());
}

#[test]
fn album_playback_sorts_tracks_and_artist_playback_keeps_aggregate_order() {
    for source in [PlaybackSource::Album, PlaybackSource::Artist] {
        let mut playback = LibraryPlayback::default();
        let mut requests = LibraryRequests::default();
        let request = playback.begin(&mut requests, source);
        let songs = playback
            .complete(request, Ok(vec![song("second", 2), song("first", 1)]))
            .unwrap()
            .unwrap();
        let ids = songs
            .iter()
            .map(|song| song.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            ids,
            if source == PlaybackSource::Album {
                ["first", "second"]
            } else {
                ["second", "first"]
            }
        );
    }
}

#[test]
fn empty_results_and_failures_consume_only_their_own_playback_intent() {
    for result in [Ok(vec![]), Err("offline".into())] {
        let mut playback = LibraryPlayback::default();
        let mut requests = LibraryRequests::default();
        let request = playback.begin(&mut requests, PlaybackSource::Album);
        assert!(playback.complete(request, result).is_some());
        assert!(playback
            .complete(request, Ok(vec![song("late", 1)]))
            .is_none());
        let retry = playback.begin(&mut requests, PlaybackSource::Album);
        assert!(playback
            .complete(retry, Ok(vec![song("retry", 1)]))
            .unwrap()
            .is_ok());
    }
}

#[test]
fn playing_album_a_and_browsing_album_b_have_independent_responses() {
    for playback_first in [true, false] {
        let mut state = AppState::default();
        let mut requests = LibraryRequests::default();
        let play_a = state
            .library_playback
            .begin(&mut requests, PlaybackSource::Album);
        state.album_details.select(Album {
            id: "B".into(),
            ..Default::default()
        });
        let browse_b = state.album_details.begin(&mut requests).unwrap();
        let mut played = None;
        for complete_playback in [playback_first, !playback_first] {
            if complete_playback {
                played = Some(
                    state
                        .library_playback
                        .complete(play_a, Ok(vec![song("A1", 1)]))
                        .unwrap()
                        .unwrap(),
                );
            } else {
                state
                    .album_details
                    .complete(browse_b, Ok(vec![song("B1", 1)]))
                    .unwrap()
                    .unwrap();
            }
        }
        assert_eq!(played.unwrap()[0].id, "A1");
        assert_eq!(state.album_details.album().unwrap().id, "B");
        assert_eq!(state.album_details.songs()[0].id, "B1");
    }
}

#[test]
fn cancelled_playback_does_not_cancel_browsing() {
    let mut state = AppState::default();
    let mut requests = LibraryRequests::default();
    state.album_details.select(Album::default());
    let details = state.album_details.begin(&mut requests).unwrap();
    let playback = state
        .library_playback
        .begin(&mut requests, PlaybackSource::Album);
    state.library_playback.cancel();
    assert!(state
        .library_playback
        .complete(playback, Ok(vec![song("play", 1)]))
        .is_none());
    assert!(state.album_details.loading());
    assert!(state
        .album_details
        .complete(details, Ok(vec![song("browse", 1)]))
        .unwrap()
        .is_ok());
    assert_eq!(state.album_details.songs()[0].id, "browse");
}

#[test]
fn resetting_server_state_rejects_outstanding_playback() {
    let mut state = AppState::default();
    let mut requests = LibraryRequests::default();
    let old = state
        .library_playback
        .begin(&mut requests, PlaybackSource::Artist);
    state = AppState::default();
    let current = state
        .library_playback
        .begin(&mut requests, PlaybackSource::Artist);
    assert_ne!(old, current);
    assert!(state
        .library_playback
        .complete(old, Ok(vec![song("old-server", 1)]))
        .is_none());
    assert!(state
        .library_playback
        .complete(old, Err("old-server error".into()))
        .is_none());
    let songs = state
        .library_playback
        .complete(current, Ok(vec![song("new-server", 1)]))
        .unwrap()
        .unwrap();
    assert_eq!(songs[0].id, "new-server");
}

#[test]
fn manual_playback_cancels_pending_playback_but_preserves_daily_generation_and_browsing() {
    let mut state = AppState::default();
    let mut requests = LibraryRequests::default();
    let library = state
        .library_playback
        .begin(&mut requests, PlaybackSource::Album);
    state.album_details.select(Album::default());
    let browse = state.album_details.begin(&mut requests).unwrap();
    let queue = PlaybackQueue::from_songs(vec![song("anchor", 1)], QueueOrigin::Manual);
    let continuation = state
        .auto_continue
        .begin("server", &queue, &mut requests)
        .unwrap();
    let daily = state
        .daily_mix
        .begin("server", "2026-11-04", 0, true, &mut requests)
        .unwrap();

    state.cancel_deferred_playback();

    assert!(state
        .library_playback
        .complete(library, Ok(vec![song("late", 1)]))
        .is_none());
    assert!(!state.auto_continue.loading());
    assert!(state.auto_continue.complete(&continuation, ()).is_none());
    assert!(state.album_details.loading());
    state
        .album_details
        .complete(browse, Ok(vec![song("browse", 1)]));
    assert_eq!(state.album_details.songs()[0].id, "browse");
    assert!(state.daily_mix.loading());
    assert_eq!(
        state.daily_mix.complete(
            &daily,
            Ok(crate::recommendation::DailyMixSnapshot {
                server_id: "server".into(),
                day: "2026-11-04".into(),
                algorithm_version: 2,
                variant: 0,
                generated_at: 0,
                tracks: Vec::new(),
            })
        ),
        Some(Ok(false))
    );
    assert!(state.daily_mix.snapshot().is_some());
}
