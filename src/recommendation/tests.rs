use super::*;
use crate::models::ListeningHistory;
use md5::{Digest, Md5};

fn song(id: &str, artist: &str, album: &str) -> Song {
    Song {
        id: id.into(),
        title: id.into(),
        artist: artist.into(),
        artist_id: Some(format!("artist-{artist}")),
        album: album.into(),
        album_id: Some(format!("album-{album}")),
        ..Song::default()
    }
}

fn candidate(song: Song, source: CandidateSource) -> RecommendationCandidate {
    RecommendationCandidate { song, source }
}

fn request(target_count: usize, seed: u64) -> RecommendationRequest {
    RecommendationRequest {
        kind: RecommendationKind::DailyMix,
        target_count,
        seed,
    }
}

#[test]
fn ranking_contract_matches_recorded_results() {
    let mut duplicate = song("duplicate", "a", "x");
    duplicate.title = "First metadata wins".into();
    let mut later_duplicate = duplicate.clone();
    later_duplicate.title = "Later metadata".into();
    let candidates = vec![
        candidate(duplicate, CandidateSource::Favorite),
        candidate(later_duplicate, CandidateSource::Random),
        candidate(song("similar", "a", "x"), CandidateSource::Similar),
        candidate(song("top", "b", "y"), CandidateSource::ArtistTop),
        candidate(song("artist", "a", "z"), CandidateSource::SameArtist),
        candidate(song("genre", "c", "y"), CandidateSource::SameGenre),
        candidate(song("random", "d", "z"), CandidateSource::Random),
        candidate(song("newest", "e", "u"), CandidateSource::Newest),
        candidate(song("frequent", "f", "v"), CandidateSource::Frequent),
        candidate(song("recent", "r", "r"), CandidateSource::Favorite),
        candidate(song("recent-similar", "r", "y"), CandidateSource::Similar),
        candidate(song("blocked", "g", "w"), CandidateSource::Favorite),
        candidate(song("  ", "h", "w"), CandidateSource::Favorite),
    ];
    let mut history = ListeningHistory::default();
    history.record_started(song("genre", "c", "y"), 1);
    assert!(history.update_latest("genre", 200, true));
    let context = RecommendationContext {
        excluded_song_ids: HashSet::from(["blocked".into()]),
        recent_song_ids: HashSet::from(["recent".into(), "recent-similar".into()]),
        listening_profile: ListeningProfile::from_history(&history),
        max_tracks_per_artist: 1,
        max_tracks_per_album: 1,
    };
    // Recorded before splitting ranking and selection. Each snapshot includes
    // ordered songs, exact scores, reasons and queue provenance across seeds,
    // small targets, recent-track fallback and exhausted candidate pools.
    for (kind, expected) in [
        (
            RecommendationKind::DailyMix,
            "0ddadfbaa003818ac1364e30d37a29d5",
        ),
        (
            RecommendationKind::SongRadio,
            "ccc338bbd451f8ceb3d4f098f456cf74",
        ),
        (
            RecommendationKind::ArtistRadio,
            "99e75f9ac1e17f537154a81a2e1d3796",
        ),
        (
            RecommendationKind::AutoContinue,
            "075a5a5eb5e23ae86609cbee40f2dc2c",
        ),
    ] {
        let mut snapshots = Vec::new();
        for seed in [7, 42, 99] {
            for target_count in [0, 1, 6, 10, 20] {
                let result = recommend(
                    &RecommendationRequest {
                        kind,
                        target_count,
                        seed,
                    },
                    candidates.clone(),
                    &context,
                );
                assert_eq!(result.len(), target_count.min(10));
                assert_eq!(
                    result
                        .iter()
                        .map(|track| &track.song.id)
                        .collect::<HashSet<_>>()
                        .len(),
                    result.len()
                );
                assert!(result
                    .iter()
                    .all(|track| !context.excluded_song_ids.contains(&track.song.id)));
                if let Some(track) = result.iter().find(|track| track.song.id == "duplicate") {
                    assert_eq!(track.song.title, "First metadata wins");
                }
                snapshots.push(result);
            }
        }
        let bytes = serde_json::to_vec(&snapshots).unwrap();
        assert_eq!(
            format!("{:x}", Md5::digest(bytes)),
            expected,
            "{kind:?}: {snapshots:?}"
        );
    }
}

#[test]
fn fresh_tracks_exhaust_relaxed_diversity_before_recent_tracks() {
    for kind in [RecommendationKind::DailyMix, RecommendationKind::SongRadio] {
        let mut candidates = (0..4)
            .map(|index| {
                candidate(
                    song(&format!("fresh-{index}"), "same", "same"),
                    CandidateSource::Random,
                )
            })
            .collect::<Vec<_>>();
        candidates.push(candidate(
            song("recent", "other", "other"),
            CandidateSource::Favorite,
        ));
        let result = recommend(
            &RecommendationRequest {
                kind,
                target_count: 4,
                seed: 42,
            },
            candidates,
            &RecommendationContext {
                recent_song_ids: HashSet::from(["recent".into()]),
                max_tracks_per_artist: 1,
                max_tracks_per_album: 1,
                ..RecommendationContext::default()
            },
        );
        assert_eq!(result.len(), 4);
        assert!(result
            .iter()
            .all(|track| track.song.id.starts_with("fresh-")));
    }
}

#[test]
fn excludes_queue_ids_and_merges_duplicate_candidate_sources() {
    let duplicate = song("a", "one", "first");
    let result = recommend(
        &request(10, 7),
        vec![
            candidate(duplicate.clone(), CandidateSource::Random),
            candidate(duplicate, CandidateSource::Favorite),
            candidate(song("blocked", "two", "second"), CandidateSource::Favorite),
            candidate(song("", "none", "none"), CandidateSource::Favorite),
        ],
        &RecommendationContext {
            excluded_song_ids: HashSet::from(["blocked".into()]),
            ..RecommendationContext::with_daily_mix_defaults()
        },
    );
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].song.id, "a");
    assert_eq!(result[0].reason, RecommendationReason::Favorite);
    assert!(result[0].score > 90.0);
}

#[test]
fn applies_artist_and_album_diversity_before_relaxing() {
    let candidates = vec![
        candidate(song("a1", "a", "x"), CandidateSource::Favorite),
        candidate(song("a2", "a", "x"), CandidateSource::Favorite),
        candidate(song("a3", "a", "x"), CandidateSource::Favorite),
        candidate(song("b1", "b", "y"), CandidateSource::Random),
        candidate(song("c1", "c", "z"), CandidateSource::Random),
    ];
    let result = recommend(
        &request(3, 3),
        candidates,
        &RecommendationContext {
            max_tracks_per_artist: 1,
            max_tracks_per_album: 1,
            ..RecommendationContext::default()
        },
    );
    assert_eq!(result.len(), 3);
    assert_eq!(
        result
            .iter()
            .map(|track| track.song.artist.as_str())
            .collect::<HashSet<_>>()
            .len(),
        3
    );
}

#[test]
fn fills_small_pools_by_relaxing_without_duplicates() {
    let result = recommend(
        &request(4, 11),
        (0..4)
            .map(|index| {
                candidate(
                    song(&format!("s{index}"), "same", "same"),
                    CandidateSource::Random,
                )
            })
            .collect(),
        &RecommendationContext {
            max_tracks_per_artist: 1,
            max_tracks_per_album: 1,
            ..RecommendationContext::default()
        },
    );
    assert_eq!(result.len(), 4);
    assert_eq!(
        result
            .iter()
            .map(|track| track.song.id.as_str())
            .collect::<HashSet<_>>()
            .len(),
        4
    );
}

#[test]
fn recent_tracks_are_deferred_until_needed() {
    let context = RecommendationContext {
        recent_song_ids: HashSet::from(["recent".into()]),
        ..RecommendationContext::with_daily_mix_defaults()
    };
    let candidates = vec![
        candidate(song("recent", "a", "a"), CandidateSource::Favorite),
        candidate(song("fresh", "b", "b"), CandidateSource::Random),
    ];
    let one = recommend(&request(1, 9), candidates.clone(), &context);
    assert_eq!(one[0].song.id, "fresh");
    let two = recommend(&request(2, 9), candidates, &context);
    assert_eq!(two.len(), 2);
    assert_eq!(two[0].song.id, "fresh");
}

#[test]
fn daily_mix_reserves_anchor_related_and_in_style_explore_capacity() {
    let mut candidates = Vec::new();
    for index in 0..10 {
        candidates.push(candidate(
            song(
                &format!("f{index}"),
                &format!("fa{index}"),
                &format!("fx{index}"),
            ),
            CandidateSource::Favorite,
        ));
        candidates.push(candidate(
            song(
                &format!("s{index}"),
                &format!("sa{index}"),
                &format!("sx{index}"),
            ),
            CandidateSource::Similar,
        ));
        candidates.push(candidate(
            song(
                &format!("e{index}"),
                &format!("ea{index}"),
                &format!("ex{index}"),
            ),
            CandidateSource::Random,
        ));
    }
    let result = recommend(
        &request(10, 5),
        candidates,
        &RecommendationContext::with_daily_mix_defaults(),
    );
    let familiar = result
        .iter()
        .filter(|track| track.reason == RecommendationReason::Favorite)
        .count();
    let related = result
        .iter()
        .filter(|track| track.reason == RecommendationReason::SimilarTrack)
        .count();
    let explore = result
        .iter()
        .filter(|track| track.reason == RecommendationReason::Explore)
        .count();
    assert_eq!((familiar, related, explore), (1, 7, 2));
}

#[test]
fn listening_profile_distinguishes_completed_engaged_and_skipped_tracks() {
    let mut history = ListeningHistory::default();
    let mut completed = song("completed", "favorite", "one");
    completed.duration = Some(200);
    completed.genre = Some("Pop".into());
    history.record_started(completed.clone(), 1);
    assert!(history.update_latest("completed", 180, true));

    let mut engaged = song("engaged", "engaged", "two");
    engaged.duration = Some(100);
    history.record_started(engaged.clone(), 2);
    assert!(history.update_latest("engaged", 50, false));

    let mut skipped = song("skipped", "skipped", "three");
    skipped.duration = Some(240);
    history.record_started(skipped.clone(), 3);
    assert!(history.update_latest("skipped", 8, false));

    let profile = ListeningProfile::from_history(&history);
    assert!(profile.score(&completed) > profile.score(&engaged));
    assert!(profile.score(&engaged) > profile.score(&skipped));
    assert!(profile.score(&skipped) < 0.0);

    let related_artist = song("new", "favorite", "new-album");
    let unrelated = song("other", "other", "other-album");
    assert!(profile.score(&related_artist) > profile.score(&unrelated));
}

#[test]
fn listening_profile_changes_order_inside_the_same_candidate_source() {
    let preferred = song("preferred", "preferred", "a");
    let neutral = song("neutral", "neutral", "b");
    let mut history = ListeningHistory::default();
    history.record_started(preferred.clone(), 1);
    assert!(history.update_latest("preferred", 200, true));
    let result = recommend(
        &request(2, 10),
        vec![
            candidate(neutral, CandidateSource::Random),
            candidate(preferred, CandidateSource::Random),
        ],
        &RecommendationContext {
            listening_profile: ListeningProfile::from_history(&history),
            ..RecommendationContext::with_daily_mix_defaults()
        },
    );
    assert_eq!(result[0].song.id, "preferred");
}

#[test]
fn ranking_is_stable_for_the_same_seed_and_varies_for_another_variant() {
    let candidates = (0..20)
        .map(|index| {
            candidate(
                song(
                    &format!("s{index}"),
                    &format!("a{index}"),
                    &format!("x{index}"),
                ),
                CandidateSource::Random,
            )
        })
        .collect::<Vec<_>>();
    let ids = |seed| {
        recommend(
            &request(8, seed),
            candidates.clone(),
            &RecommendationContext::with_daily_mix_defaults(),
        )
        .into_iter()
        .map(|track| track.song.id)
        .collect::<Vec<_>>()
    };
    assert_eq!(ids(42), ids(42));
    assert_ne!(ids(42), ids(43));
}
