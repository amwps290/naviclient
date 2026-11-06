use super::album_plan::MAX_ALBUM_REQUESTS;
use super::*;
use crate::models::{Album, Song};
use crate::recommendation::{ListeningProfile, RecommendedTrack};
use std::collections::BTreeSet;

fn album(id: &str) -> Album {
    Album {
        id: id.into(),
        name: id.into(),
        ..Album::default()
    }
}

fn styled_song(id: &str, artist: &str, genre: &str) -> Song {
    Song {
        id: id.into(),
        title: id.into(),
        artist: artist.into(),
        artist_id: Some(format!("artist-{artist}")),
        genre: Some(genre.into()),
        ..Song::default()
    }
}

#[test]
fn style_seeds_stay_inside_the_primary_genre_cluster() {
    let favorites = vec![
        styled_song("a-pop", "one", "Pop"),
        styled_song("b-pop", "two", "pop"),
        styled_song("c-rock", "three", "Rock"),
    ];
    let seeds = select_style_seeds(&favorites, &[], &ListeningProfile::default());
    assert_eq!(
        seeds
            .iter()
            .map(|song| song.id.as_str())
            .collect::<Vec<_>>(),
        vec!["a-pop", "b-pop"]
    );
}

#[test]
fn exploration_requires_the_seed_style_when_metadata_is_available() {
    let seeds = vec![styled_song("seed", "one", "Ambient")];
    let style = StyleSignature::from_seeds(&seeds);
    let profile = ListeningProfile::default();
    assert!(style.matches(&styled_song("same", "two", "ambient"), &profile));
    assert!(style.matches(&styled_song("artist", "one", "Rock"), &profile));
    assert!(!style.matches(&styled_song("off", "three", "Metal"), &profile));
}

#[test]
fn album_plan_is_bounded_deduplicated_and_keeps_all_sources() {
    let plan = merge_album_plan([
        (
            vec![album("shared"), album("frequent")],
            CandidateSource::Frequent,
        ),
        (vec![album("shared"), album("new")], CandidateSource::Newest),
        (
            (0..20).map(|index| album(&format!("r{index}"))).collect(),
            CandidateSource::Random,
        ),
    ]);
    assert_eq!(plan.len(), MAX_ALBUM_REQUESTS);
    assert_eq!(plan[0].id, "shared");
    assert_eq!(
        plan[0].sources,
        BTreeSet::from([CandidateSource::Frequent, CandidateSource::Newest])
    );
}

#[test]
fn full_album_plans_still_merge_later_sources_without_admitting_new_ids() {
    let plan = merge_album_plan([
        (
            (0..MAX_ALBUM_REQUESTS)
                .map(|i| album(&i.to_string()))
                .collect(),
            CandidateSource::Frequent,
        ),
        (
            vec![album("overflow"), album("9"), album("0"), album(" ")],
            CandidateSource::Newest,
        ),
        (vec![album("9"), album("9")], CandidateSource::Random),
    ]);
    assert_eq!(
        plan.iter()
            .map(|request| request.id.as_str())
            .collect::<Vec<_>>(),
        (0..MAX_ALBUM_REQUESTS)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        plan[0].sources,
        BTreeSet::from([CandidateSource::Frequent, CandidateSource::Newest])
    );
    assert_eq!(
        plan[9].sources,
        BTreeSet::from([
            CandidateSource::Frequent,
            CandidateSource::Newest,
            CandidateSource::Random,
        ])
    );
}

#[test]
fn style_name_fallback_ignores_blank_ids_but_respects_nonblank_ids() {
    let mut seed = styled_song("seed", " One ", " ");
    seed.artist_id = Some(" ".into());
    let style = StyleSignature::from_seeds(&[seed]);
    let profile = ListeningProfile::default();
    let mut matching = styled_song("matching", "ONE", "");
    matching.artist_id = None;
    assert!(style.matches(&matching, &profile));
    matching.artist_id = Some("different-id".into());
    assert!(!style.matches(&matching, &profile));
}

#[test]
fn style_seed_limit_keeps_the_primary_and_ignores_exploration_sources() {
    let favorites = vec![styled_song("favorite", "one", "Ambient")];
    let mut candidates = (0..8)
        .map(|i| RecommendationCandidate {
            song: styled_song(&format!("frequent-{i}"), "other", "ambient"),
            source: CandidateSource::Frequent,
        })
        .collect::<Vec<_>>();
    candidates.push(RecommendationCandidate {
        song: styled_song("explore", "one", "Ambient"),
        source: CandidateSource::Newest,
    });
    let seeds = select_style_seeds(&favorites, &candidates, &ListeningProfile::default());
    assert_eq!(
        seeds
            .iter()
            .map(|song| song.id.as_str())
            .collect::<Vec<_>>(),
        ["favorite", "frequent-0", "frequent-1", "frequent-2"]
    );
}

#[test]
fn daily_seed_is_stable_and_variant_sensitive() {
    assert_eq!(
        daily_seed("server", "2026-11-02", 1, 0),
        daily_seed("server", "2026-11-02", 1, 0)
    );
    assert_ne!(
        daily_seed("server", "2026-11-02", 1, 0),
        daily_seed("server", "2026-11-02", 1, 1)
    );
    assert_ne!(
        daily_seed("server-a", "2026-11-02", 1, 0),
        daily_seed("server-b", "2026-11-02", 1, 0)
    );
}

#[test]
fn snapshot_round_trips_with_recommendation_reasons() {
    let snapshot = DailyMixSnapshot {
        server_id: "server".into(),
        day: "2026-11-02".into(),
        algorithm_version: DAILY_MIX_VERSION,
        variant: 2,
        generated_at: 123,
        tracks: vec![RecommendedTrack {
            song: Song {
                id: "song".into(),
                title: "Song".into(),
                ..Song::default()
            },
            reason: super::super::RecommendationReason::Explore,
            score: 15.5,
            origin: crate::models::QueueOrigin::DailyMix,
        }],
    };
    let json = serde_json::to_string(&snapshot).unwrap();
    let restored: DailyMixSnapshot = serde_json::from_str(&json).unwrap();
    assert_eq!(restored.variant, 2);
    assert_eq!(restored.tracks[0].song.id, "song");
    assert_eq!(
        restored.tracks[0].reason,
        super::super::RecommendationReason::Explore
    );
}

#[test]
fn snapshot_validity_requires_server_day_and_algorithm_version() {
    let snapshot = DailyMixSnapshot {
        server_id: "server".into(),
        day: "2026-11-02".into(),
        algorithm_version: DAILY_MIX_VERSION,
        variant: 0,
        generated_at: 0,
        tracks: Vec::new(),
    };
    assert!(snapshot.is_current("server", "2026-11-02"));
    assert!(!snapshot.is_current("other", "2026-11-02"));
    assert!(!snapshot.is_current("server", "2026-11-03"));
    let old = DailyMixSnapshot {
        algorithm_version: DAILY_MIX_VERSION - 1,
        ..snapshot
    };
    assert!(!old.is_current("server", "2026-11-02"));
}
