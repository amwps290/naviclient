use super::*;
use crate::models::Artist;
use crate::recommendation::RecommendationReason;

fn song(id: &str) -> Song {
    Song {
        id: id.to_string(),
        ..Song::default()
    }
}

fn artist(id: &str) -> Artist {
    Artist {
        id: id.to_string(),
        ..Artist::default()
    }
}

fn tracks(origin: QueueOrigin) -> Vec<RecommendedTrack> {
    vec![RecommendedTrack {
        song: song("recommended"),
        reason: RecommendationReason::SimilarTrack,
        score: 2.0,
        origin,
    }]
}

fn finish(
    state: &mut RadioState,
    request: &RadioRequest,
    result: Result<Vec<RecommendedTrack>, String>,
) -> Option<Result<Vec<RecommendedTrack>, String>> {
    let initial_song = song(&request.initial_song_id);
    let queue = PlaybackQueue::from_songs(vec![initial_song.clone()], request.origin());
    state.complete(
        request,
        Some(&request.server_id),
        Some(0),
        &queue,
        Some(&initial_song),
        result,
    )
}

#[test]
fn typed_targets_distinguish_identical_ids_and_do_not_parse_id_prefixes() {
    let mut requests = LibraryRequests::default();
    let mut state = RadioState::default();
    for id in ["same", "artist:seed", "song:seed"] {
        let initial_song = song(id);
        let request = state.begin(
            "server",
            &RadioSeed::Song(Box::new(initial_song.clone())),
            &initial_song,
            &mut requests,
        );
        assert!(state.song_loading(id));
        assert!(!state.artist_loading(id));
        assert!(!state.song_loading("different"));
        assert!(!request.is_artist());
        assert_eq!(request.origin(), QueueOrigin::SongRadio);

        let request = state.begin(
            "server",
            &RadioSeed::Artist(artist(id)),
            &initial_song,
            &mut requests,
        );
        assert!(state.artist_loading(id));
        assert!(!state.song_loading(id));
        assert!(request.is_artist());
        assert_eq!(request.origin(), QueueOrigin::ArtistRadio);
    }
}

#[test]
fn returning_to_a_seed_rejects_previous_success_and_failure() {
    let mut requests = LibraryRequests::default();
    let mut state = RadioState::default();
    let a = song("a");
    let old_a = state.begin(
        "server",
        &RadioSeed::Song(Box::new(a.clone())),
        &a,
        &mut requests,
    );
    let b = state.begin(
        "server",
        &RadioSeed::Artist(artist("b")),
        &song("b-track"),
        &mut requests,
    );
    let current_a = state.begin(
        "server",
        &RadioSeed::Song(Box::new(a.clone())),
        &a,
        &mut requests,
    );
    assert!(finish(&mut state, &old_a, Ok(tracks(QueueOrigin::SongRadio))).is_none());
    assert!(finish(&mut state, &old_a, Err("old failure".into())).is_none());
    assert!(finish(&mut state, &b, Ok(tracks(QueueOrigin::ArtistRadio))).is_none());
    assert!(state.song_loading("a"));
    assert!(!state.artist_loading("b"));
    let accepted = finish(&mut state, &current_a, Ok(tracks(QueueOrigin::SongRadio)))
        .unwrap()
        .unwrap();
    assert_eq!(accepted[0].song.id, "recommended");
    assert!(!state.song_loading("a"));
}

#[test]
fn cancelling_then_restarting_the_same_seed_keeps_old_responses_from_consuming_new_request() {
    let mut requests = LibraryRequests::default();
    let mut state = RadioState::default();
    let seed = RadioSeed::Artist(artist("artist"));
    let initial_song = song("initial");
    let old = state.begin("server", &seed, &initial_song, &mut requests);
    state.cancel();
    assert!(!state.artist_loading("artist"));
    assert!(finish(&mut state, &old, Ok(tracks(QueueOrigin::ArtistRadio))).is_none());
    let current = state.begin("server", &seed, &initial_song, &mut requests);
    assert!(finish(&mut state, &old, Err("cancelled failure".into())).is_none());
    assert!(state.artist_loading("artist"));
    assert!(finish(&mut state, &current, Ok(tracks(QueueOrigin::ArtistRadio))).is_some());
}

#[test]
fn completed_empty_failed_and_successful_requests_are_consumed_once() {
    let mut requests = LibraryRequests::default();
    let mut state = RadioState::default();
    let initial_song = song("initial");
    let seed = RadioSeed::Song(Box::new(initial_song.clone()));
    for result in [
        Ok(Vec::new()),
        Err("offline".into()),
        Ok(tracks(QueueOrigin::SongRadio)),
    ] {
        let old = state.begin("server", &seed, &initial_song, &mut requests);
        assert!(finish(&mut state, &old, result).is_some());
        assert!(!state.song_loading("initial"));
        let current = state.begin("server", &seed, &initial_song, &mut requests);
        assert!(finish(&mut state, &old, Ok(tracks(QueueOrigin::SongRadio))).is_none());
        assert!(finish(&mut state, &old, Err("duplicate failure".into())).is_none());
        assert!(state.song_loading("initial"));
        assert!(finish(&mut state, &current, Ok(Vec::new())).is_some());
    }
}

#[test]
fn wrong_server_response_does_not_clear_pending_request() {
    let mut requests = LibraryRequests::default();
    let mut state = RadioState::default();
    let initial_song = song("initial");
    let request = state.begin(
        "server",
        &RadioSeed::Song(Box::new(initial_song.clone())),
        &initial_song,
        &mut requests,
    );
    let queue = PlaybackQueue::from_songs(vec![initial_song.clone()], request.origin());
    for server in [None, Some("other")] {
        assert!(state
            .complete(
                &request,
                server,
                Some(0),
                &queue,
                Some(&initial_song),
                Ok(Vec::new())
            )
            .is_none());
        assert!(state.song_loading("initial"));
    }
    assert!(finish(&mut state, &request, Ok(Vec::new())).is_some());
}

#[test]
fn changed_seed_queue_or_stopped_playback_discards_result_and_finishes_loading() {
    let mut requests = LibraryRequests::default();
    for (ids, index, now_playing) in [
        (vec![], Some(0), Some("initial")),
        (vec!["initial", "manual"], Some(0), Some("initial")),
        (vec!["other"], Some(0), Some("initial")),
        (vec!["initial"], Some(1), Some("initial")),
        (vec!["initial"], None, Some("initial")),
        (vec!["initial"], Some(0), Some("other")),
        (vec!["initial"], Some(0), None),
    ] {
        let mut state = RadioState::default();
        let initial_song = song("initial");
        let request = state.begin(
            "server",
            &RadioSeed::Song(Box::new(initial_song.clone())),
            &initial_song,
            &mut requests,
        );
        let queue =
            PlaybackQueue::from_songs(ids.into_iter().map(song).collect(), request.origin());
        let now_playing = now_playing.map(song);
        assert!(state
            .complete(
                &request,
                Some("server"),
                index,
                &queue,
                now_playing.as_ref(),
                Ok(tracks(request.origin()))
            )
            .is_none());
        assert!(!state.song_loading("initial"));
        // Restoring the original queue after rejecting a result cannot revive it.
        assert!(finish(&mut state, &request, Ok(tracks(request.origin()))).is_none());
    }
}

#[test]
fn resetting_server_state_rejects_old_radio_with_reused_ids() {
    let mut requests = LibraryRequests::default();
    let mut state = crate::app::AppState::default();
    let seed = RadioSeed::Artist(artist("artist"));
    let initial_song = song("initial");
    let old = state
        .radio
        .begin("server", &seed, &initial_song, &mut requests);
    state = crate::app::AppState::default();
    let current = state
        .radio
        .begin("server", &seed, &initial_song, &mut requests);
    assert!(finish(&mut state.radio, &old, Ok(tracks(QueueOrigin::ArtistRadio))).is_none());
    assert!(finish(&mut state.radio, &old, Err("old server failure".into())).is_none());
    assert!(state.radio.artist_loading("artist"));
    let accepted = finish(
        &mut state.radio,
        &current,
        Ok(tracks(QueueOrigin::ArtistRadio)),
    )
    .unwrap()
    .unwrap();
    assert_eq!(accepted[0].origin, QueueOrigin::ArtistRadio);
}

#[test]
fn browsing_a_different_artist_does_not_change_radio_request_or_seed_queue() {
    let mut requests = LibraryRequests::default();
    let mut state = crate::app::AppState::default();
    let initial_song = song("initial");
    let request = state.radio.begin(
        "server",
        &RadioSeed::Artist(artist("a")),
        &initial_song,
        &mut requests,
    );
    state.queue = PlaybackQueue::from_songs(vec![initial_song.clone()], request.origin());
    state.queue_index = Some(0);
    state.now_playing = Some(initial_song);
    state.artist_details.select(artist("b"));
    assert!(state.radio.artist_loading("a"));
    assert!(!state.radio.artist_loading("b"));
    let accepted = state
        .radio
        .complete(
            &request,
            Some("server"),
            state.queue_index,
            &state.queue,
            state.now_playing.as_ref(),
            Ok(tracks(request.origin())),
        )
        .unwrap()
        .unwrap();
    assert_eq!(accepted[0].origin, QueueOrigin::ArtistRadio);
    assert_eq!(state.artist_details.artist().unwrap().id, "b");
    assert_eq!(state.queue.song(0).unwrap().id, "initial");
}
