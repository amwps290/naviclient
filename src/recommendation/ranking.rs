//! Candidate merging, source scoring and deterministic ordering.

use std::collections::{BTreeSet, HashMap};

use md5::{Digest, Md5};

use super::{
    CandidateSource, RecommendationCandidate, RecommendationContext, RecommendationKind,
    RecommendationReason, RecommendationRequest,
};
use crate::models::Song;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MixBucket {
    Familiar,
    Related,
    Explore,
}

pub(super) struct RankedCandidate {
    pub(super) song: Song,
    pub(super) reason: RecommendationReason,
    pub(super) bucket: MixBucket,
    pub(super) score: f32,
    pub(super) recent: bool,
}

pub(super) fn rank(
    request: &RecommendationRequest,
    candidates: Vec<RecommendationCandidate>,
    context: &RecommendationContext,
) -> Vec<RankedCandidate> {
    let mut merged: HashMap<String, (Song, BTreeSet<CandidateSource>)> = HashMap::new();
    for candidate in candidates {
        let id = candidate.song.id.trim();
        if id.is_empty() || context.excluded_song_ids.contains(id) {
            continue;
        }
        merged
            .entry(candidate.song.id.clone())
            .and_modify(|(_, sources)| {
                sources.insert(candidate.source);
            })
            .or_insert_with(|| {
                let mut sources = BTreeSet::new();
                sources.insert(candidate.source);
                (candidate.song, sources)
            });
    }

    let mut ranked = merged
        .into_values()
        .map(|(song, sources)| {
            let strongest = sources
                .iter()
                .copied()
                .max_by_key(|source| source_weight(request.kind, *source))
                .unwrap_or(CandidateSource::Random);
            let strongest_score = source_weight(request.kind, strongest) as f32;
            let supporting_score: f32 = sources
                .iter()
                .copied()
                .filter(|source| *source != strongest)
                .map(|source| source_weight(request.kind, source) as f32 * 0.2)
                .sum();
            let score = strongest_score
                + supporting_score
                + context.listening_profile.score(&song)
                + stable_jitter(request.seed, &song.id) * 0.99;
            let recent = context.recent_song_ids.contains(&song.id);
            RankedCandidate {
                song,
                reason: source_reason(strongest),
                bucket: source_bucket(strongest),
                score,
                recent,
            }
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.song.id.cmp(&right.song.id))
    });

    ranked
}

fn source_weight(kind: RecommendationKind, source: CandidateSource) -> u16 {
    match (kind, source) {
        (RecommendationKind::AutoContinue, CandidateSource::Similar) => 105,
        (RecommendationKind::AutoContinue, CandidateSource::SameArtist) => 80,
        (RecommendationKind::AutoContinue, CandidateSource::SameGenre) => 70,
        (RecommendationKind::AutoContinue, CandidateSource::Favorite) => 55,
        (_, CandidateSource::Favorite) => 90,
        (RecommendationKind::SongRadio, CandidateSource::Similar) => 100,
        (RecommendationKind::ArtistRadio, CandidateSource::ArtistTop) => 100,
        (_, CandidateSource::Similar) => 75,
        (_, CandidateSource::Frequent) => 65,
        (_, CandidateSource::ArtistTop) => 60,
        (RecommendationKind::ArtistRadio, CandidateSource::SameArtist) => 80,
        (_, CandidateSource::SameArtist) => 50,
        (_, CandidateSource::SameGenre) => 40,
        (_, CandidateSource::Newest) => 25,
        (_, CandidateSource::Random) => 15,
    }
}

fn source_bucket(source: CandidateSource) -> MixBucket {
    match source {
        CandidateSource::Favorite | CandidateSource::Frequent => MixBucket::Familiar,
        CandidateSource::Similar
        | CandidateSource::ArtistTop
        | CandidateSource::SameArtist
        | CandidateSource::SameGenre => MixBucket::Related,
        CandidateSource::Newest | CandidateSource::Random => MixBucket::Explore,
    }
}

fn source_reason(source: CandidateSource) -> RecommendationReason {
    match source {
        CandidateSource::Favorite => RecommendationReason::Favorite,
        CandidateSource::Frequent => RecommendationReason::FrequentlyPlayed,
        CandidateSource::Similar => RecommendationReason::SimilarTrack,
        CandidateSource::ArtistTop => RecommendationReason::PopularByArtist,
        CandidateSource::SameArtist => RecommendationReason::SameArtist,
        CandidateSource::SameGenre => RecommendationReason::SameGenre,
        CandidateSource::Newest => RecommendationReason::NewRelease,
        CandidateSource::Random => RecommendationReason::Explore,
    }
}

fn stable_jitter(seed: u64, song_id: &str) -> f32 {
    let mut hasher = Md5::new();
    hasher.update(seed.to_le_bytes());
    hasher.update(song_id.as_bytes());
    let digest = hasher.finalize();
    let value = u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]]);
    value as f32 / u32::MAX as f32
}
