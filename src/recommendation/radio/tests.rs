use super::super::{
    CandidateSource, ListeningProfile, RecommendationContext, RecommendationReason,
};
use super::*;
use crate::models::QueueOrigin;
use std::collections::{BTreeMap, HashSet};

use crate::models::Album;
use crate::test_support::http::{success, Request, Server};

fn song(id: &str) -> Song {
    Song {
        id: id.into(),
        title: id.into(),
        artist: "Artist".into(),
        artist_id: Some("artist".into()),
        ..Song::default()
    }
}

#[test]
fn radio_keys_include_the_seed_kind() {
    let artist = Artist {
        id: "same".into(),
        name: "Artist".into(),
        ..Artist::default()
    };
    assert_eq!(RadioSeed::Song(Box::new(song("same"))).key(), "song:same");
    assert_eq!(RadioSeed::Artist(artist).key(), "artist:same");
}

#[test]
fn ranker_attaches_the_requested_radio_origin() {
    let tracks = recommend(
        &RecommendationRequest {
            kind: RecommendationKind::ArtistRadio,
            target_count: 1,
            seed: 1,
        },
        vec![RecommendationCandidate {
            song: song("track"),
            source: CandidateSource::ArtistTop,
        }],
        &RecommendationContext::default(),
    );
    assert_eq!(tracks[0].origin, QueueOrigin::ArtistRadio);
}

fn context(seed: RadioSeed) -> RadioGenerationContext {
    RadioGenerationContext {
        common: RecommendationGenerationContext {
            server_id: "fixture-server".into(),
            sonic_similarity: CapabilitySupport::Supported,
            recent_song_ids: HashSet::new(),
            listening_profile: ListeningProfile::default(),
            excluded_song_ids: HashSet::from(["seed".into()]),
        },
        seed,
        top_songs_by_artist_id: CapabilitySupport::Supported,
    }
}

fn seeds() -> [RadioSeed; 2] {
    [
        RadioSeed::Song(Box::new(song("seed"))),
        RadioSeed::Artist(Artist {
            id: "artist".into(),
            name: "Artist Name".into(),
            ..Default::default()
        }),
    ]
}

fn candidate(id: &str) -> RecommendationCandidate {
    RecommendationCandidate {
        song: song(id),
        source: CandidateSource::SameArtist,
    }
}

#[test]
fn empty_collection_preserves_source_error_order_and_has_a_distinct_no_facts_error() {
    for seed in seeds() {
        assert_eq!(
            context(seed.clone())
                .select(vec![], vec![])
                .unwrap_err()
                .to_string(),
            "the server returned no radio candidates"
        );
        assert_eq!(
            context(seed)
                .select(vec![], vec!["first source".into(), "second source".into()])
                .unwrap_err()
                .to_string(),
            "first source; second source"
        );
    }
}

#[test]
fn selection_keeps_24_unique_fresh_tracks_queue_exclusions_metadata_and_radio_origins() {
    for seed in seeds() {
        let expected_origin = if matches!(seed, RadioSeed::Song(_)) {
            QueueOrigin::SongRadio
        } else {
            QueueOrigin::ArtistRadio
        };
        let mut context = context(seed);
        context
            .common
            .excluded_song_ids
            .extend(["track0".into(), "track1".into()]);
        context
            .common
            .recent_song_ids
            .extend((2..14).map(|id| format!("track{id}")));
        let mut candidates: Vec<_> = (0..40).map(|id| candidate(&format!("track{id}"))).collect();
        let mut duplicate = candidate("track20");
        duplicate.song.title = "later duplicate metadata".into();
        duplicate.source = CandidateSource::Similar;
        candidates.push(duplicate);
        let tracks = context.select(candidates, vec![]).unwrap();
        assert_eq!(tracks.len(), 24);
        assert_eq!(
            tracks
                .iter()
                .map(|track| &track.song.id)
                .collect::<HashSet<_>>()
                .len(),
            24
        );
        assert!(tracks.iter().all(|track| track.origin == expected_origin
            && track.song.id != "track0"
            && track.song.id != "track1"));
        assert!(tracks
            .iter()
            .all(|track| !(2..14).any(|id| track.song.id == format!("track{id}"))));
        let duplicate = tracks
            .iter()
            .find(|track| track.song.id == "track20")
            .unwrap();
        assert_eq!(duplicate.song.title, "track20");
        assert_eq!(
            duplicate.reason,
            if expected_origin == QueueOrigin::SongRadio {
                RecommendationReason::SimilarTrack
            } else {
                RecommendationReason::SameArtist
            }
        );
    }
}

#[test]
fn candidate_cap_is_applied_before_exclusion_and_filtering_does_not_refill_the_pool() {
    for excluded in [179, 180] {
        let mut context = context(seeds()[0].clone());
        context
            .common
            .excluded_song_ids
            .extend((0..excluded).map(|id| format!("track{id}")));
        let candidates = (0..181)
            .map(|id| candidate(&format!("track{id}")))
            .collect();
        let result = context.select(candidates, vec!["a source failed".into()]);
        if excluded == 180 {
            assert_eq!(
                result.unwrap_err().to_string(),
                "all radio candidates were filtered out"
            );
        } else {
            let tracks = result.unwrap();
            assert_eq!(tracks.len(), 1);
            assert_eq!(tracks[0].song.id, "track179");
        }
    }
}

#[test]
fn partial_source_failures_keep_successful_results_and_repeat_the_same_ranking() {
    for seed in seeds() {
        let context = context(seed);
        let candidates: Vec<_> = (0..30).map(|id| candidate(&format!("track{id}"))).collect();
        let first = context
            .clone()
            .select(candidates.clone(), vec!["failed source".into()])
            .unwrap();
        let second = context.select(candidates, vec![]).unwrap();
        assert_eq!(
            serde_json::to_value(first).unwrap(),
            serde_json::to_value(second).unwrap()
        );
    }
}

fn api(server: &Server) -> MusicSource {
    MusicSource::opensubsonic("fixture-server", &server.url(""), "fixture", "fixture").unwrap()
}

fn endpoint(request: &Request) -> &str {
    request
        .url
        .path()
        .trim_start_matches("/rest/")
        .trim_end_matches(".view")
}

fn params(request: &Request) -> BTreeMap<String, String> {
    request.url.query_pairs().into_owned().collect()
}

fn ok(mut payload: serde_json::Value) -> (u16, String) {
    let object = payload.as_object_mut().unwrap();
    object.insert("status".into(), "ok".into());
    object.insert("version".into(), "1.16.1".into());
    success(serde_json::json!({"subsonic-response": payload}))
}

fn failure() -> (u16, String) {
    success(
        serde_json::json!({"subsonic-response": {"status": "failed", "error": {"code": 50, "message": "fixture source unavailable"}}}),
    )
}

fn albums(count: usize) -> serde_json::Value {
    serde_json::json!({"artist": {"album": (0..count).map(|index| Album { id: format!("album{index}"), ..Default::default() }).collect::<Vec<_>>()}})
}

fn album_songs(id: &str) -> serde_json::Value {
    serde_json::json!({"album": {"song": (0..15).map(|index| song(&format!("{id}-{index}"))).collect::<Vec<_>>()}})
}

#[tokio::test]
async fn song_radio_capabilities_and_album_sampling_keep_request_limits_and_source_fallbacks() {
    for support in [
        CapabilitySupport::Supported,
        CapabilitySupport::Unknown,
        CapabilitySupport::Unsupported,
    ] {
        let server = Server::with_handler(|request| match endpoint(request) {
            "getSimilarSongs2" => {
                ok(serde_json::json!({"similarSongs2": {"song": [song("seed"), song("similar")]}}))
            }
            "getArtist" => ok(albums(10)),
            "getAlbum" => ok(album_songs(&params(request)["id"])),
            "getSongsByGenre" => {
                ok(serde_json::json!({"songsByGenre": {"song": [song("genre1"), song("genre2")]}}))
            }
            _ => failure(),
        });
        let mut seed = song("seed");
        seed.genre = Some("Jazz & Soul".into());
        let mut context = context(RadioSeed::Song(Box::new(seed)));
        context.common.sonic_similarity = support;
        let (candidates, errors) = collection::collect(
            api(&server),
            &context.seed,
            support,
            context.top_songs_by_artist_id,
        )
        .await;
        assert!(errors.is_empty());
        let similar_count = if support == CapabilitySupport::Unsupported {
            0
        } else {
            2
        };
        assert_eq!(candidates.len(), similar_count + 8 * 12 + 2);
        assert!(candidates[..similar_count]
            .iter()
            .all(|candidate| candidate.source == CandidateSource::Similar));
        assert!(candidates[similar_count..similar_count + 96]
            .iter()
            .all(|candidate| candidate.source == CandidateSource::SameArtist));
        assert!(candidates[similar_count + 96..]
            .iter()
            .all(|candidate| candidate.source == CandidateSource::SameGenre));
        for album in 0..8 {
            assert_eq!(
                candidates
                    .iter()
                    .filter(|candidate| candidate.song.id.starts_with(&format!("album{album}-")))
                    .count(),
                12
            );
        }
        let tracks = context.select(candidates, errors).unwrap();
        assert_eq!(tracks.len(), 24);
        assert!(tracks
            .iter()
            .all(|track| track.song.id != "seed" && track.origin == QueueOrigin::SongRadio));
        let requests = server.requests();
        assert_eq!(requests.len(), 10 + usize::from(similar_count > 0));
        assert_eq!(
            requests
                .iter()
                .filter(|request| endpoint(request) == "getAlbum")
                .map(|request| params(request)["id"].clone())
                .collect::<HashSet<_>>(),
            (0..8).map(|id| format!("album{id}")).collect()
        );
        for request in requests {
            assert_eq!(request.method, "GET");
            let params = params(&request);
            match endpoint(&request) {
                "getSimilarSongs2" => {
                    assert_eq!(params["id"], "seed");
                    assert_eq!(params["count"], "48");
                }
                "getArtist" => assert_eq!(params["id"], "artist"),
                "getSongsByGenre" => {
                    assert_eq!(params["genre"], "Jazz & Soul");
                    assert_eq!(params["count"], "48");
                    assert_eq!(params["offset"], "0");
                }
                _ => {}
            }
        }
    }
}

#[tokio::test]
async fn artist_radio_keeps_top_song_id_fallback_and_bounded_similarity_and_genre_seeds() {
    for (similarity, top_id) in [
        (CapabilitySupport::Supported, CapabilitySupport::Supported),
        (CapabilitySupport::Unknown, CapabilitySupport::Unknown),
        (CapabilitySupport::Unsupported, CapabilitySupport::Supported),
    ] {
        let server = Server::with_handler(|request| match endpoint(request) {
            "getTopSongs" => {
                let songs = [" Jazz & Soul ", "jazz & soul", "Ambient", "Metal"]
                    .into_iter()
                    .enumerate()
                    .map(|(id, genre)| Song {
                        genre: Some(genre.into()),
                        ..song(&format!("top{id}"))
                    })
                    .collect::<Vec<_>>();
                ok(serde_json::json!({"topSongs": {"song": songs}}))
            }
            "getArtist" => ok(albums(10)),
            "getAlbum" => ok(album_songs(&params(request)["id"])),
            "getSimilarSongs2" => ok(
                serde_json::json!({"similarSongs2": {"song": [song(&format!("similar-{}", params(request)["id"]))]}}),
            ),
            "getSongsByGenre" => ok(
                serde_json::json!({"songsByGenre": {"song": [song(&format!("genre-{}", params(request)["genre"]))]}}),
            ),
            _ => failure(),
        });
        let mut context = context(seeds()[1].clone());
        context.common.sonic_similarity = similarity;
        context.top_songs_by_artist_id = top_id;
        let (candidates, errors) =
            collection::collect(api(&server), &context.seed, similarity, top_id).await;
        assert!(errors.is_empty());
        let similar_count = if similarity == CapabilitySupport::Unsupported {
            0
        } else {
            3
        };
        assert_eq!(candidates.len(), 4 + 96 + similar_count + 2);
        assert!(candidates[..4]
            .iter()
            .all(|candidate| candidate.source == CandidateSource::ArtistTop));
        assert!(candidates[4..100]
            .iter()
            .all(|candidate| candidate.source == CandidateSource::SameArtist));
        assert!(candidates[100..100 + similar_count]
            .iter()
            .all(|candidate| candidate.source == CandidateSource::Similar));
        assert!(candidates[100 + similar_count..]
            .iter()
            .all(|candidate| candidate.source == CandidateSource::SameGenre));
        assert!(context
            .select(candidates, errors)
            .unwrap()
            .iter()
            .all(|track| track.origin == QueueOrigin::ArtistRadio));
        let requests = server.requests();
        assert_eq!(requests.len(), 12 + similar_count);
        let top = requests
            .iter()
            .find(|request| endpoint(request) == "getTopSongs")
            .unwrap();
        let top_params = params(top);
        assert_eq!(top_params["count"], "36");
        if top_id == CapabilitySupport::Supported {
            assert_eq!(top_params["artistId"], "artist");
            assert!(!top_params.contains_key("artist"));
        } else {
            assert_eq!(top_params["artist"], "Artist Name");
            assert!(!top_params.contains_key("artistId"));
        }
        let genre_requests: Vec<_> = requests
            .iter()
            .filter(|request| endpoint(request) == "getSongsByGenre")
            .collect();
        assert_eq!(
            genre_requests
                .iter()
                .map(|request| params(request)["genre"].clone())
                .collect::<HashSet<_>>(),
            HashSet::from(["Jazz & Soul".into(), "Ambient".into()])
        );
        for request in genre_requests {
            assert_eq!(params(request)["count"], "30");
            assert_eq!(params(request)["offset"], "0");
        }
        let similar_requests: Vec<_> = requests
            .iter()
            .filter(|request| endpoint(request) == "getSimilarSongs2")
            .collect();
        assert_eq!(similar_requests.len(), similar_count);
        if similar_count > 0 {
            assert_eq!(
                similar_requests
                    .iter()
                    .map(|request| params(request)["id"].clone())
                    .collect::<HashSet<_>>(),
                HashSet::from(["top0".into(), "top1".into(), "top2".into()])
            );
        }
        for request in similar_requests {
            assert_eq!(params(request)["count"], "24");
        }
    }
}

#[tokio::test]
async fn generation_preserves_partial_source_fallbacks_error_order_and_optional_empty_sources() {
    for all_failed in [false, true] {
        let server = Server::with_handler(move |request| {
            if !all_failed && endpoint(request) == "getSongsByGenre" {
                ok(serde_json::json!({"songsByGenre": {"song": [song("genre")]}}))
            } else {
                failure()
            }
        });
        let mut seed = song("seed");
        seed.genre = Some("Jazz".into());
        let result = generate_radio(api(&server), context(RadioSeed::Song(Box::new(seed)))).await;
        if all_failed {
            let error = result.unwrap_err().to_string();
            assert!(error.starts_with("similar songs:"));
            assert!(error.find("artist albums:").unwrap() < error.find("genre songs:").unwrap());
        } else {
            let tracks = result.unwrap();
            assert_eq!(tracks.len(), 1);
            assert_eq!(tracks[0].song.id, "genre");
            assert_eq!(tracks[0].reason, RecommendationReason::SameGenre);
        }
        assert_eq!(server.requests().len(), 3);
    }
    let server = Server::with_handler(|request| match endpoint(request) {
        "getArtist" => ok(albums(2)),
        "getAlbum" => ok(album_songs(&params(request)["id"])),
        _ => failure(),
    });
    let tracks = generate_radio(api(&server), context(seeds()[1].clone()))
        .await
        .unwrap();
    assert_eq!(tracks.len(), 24);
    assert!(tracks
        .iter()
        .all(|track| track.origin == QueueOrigin::ArtistRadio
            && track.reason == RecommendationReason::SameArtist));
    assert_eq!(server.requests().len(), 4);

    let server = Server::new(vec![]);
    let mut context = context(RadioSeed::Song(Box::new(Song {
        artist_id: Some(" ".into()),
        genre: Some(" ".into()),
        ..song("seed")
    })));
    context.common.sonic_similarity = CapabilitySupport::Unsupported;
    assert_eq!(
        generate_radio(api(&server), context)
            .await
            .unwrap_err()
            .to_string(),
        "the server returned no radio candidates"
    );
    assert!(server.requests().is_empty());
}
