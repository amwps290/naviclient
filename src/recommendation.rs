//! Shared recommendation contracts and pure orchestration for daily mixes,
//! seed radios and auto-continue. Providers collect facts; profile, ranking and
//! selection modules own listening affinity, scoring and diversity constraints.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::models::{CapabilitySupport, QueueOrigin, Song};

mod auto_continue;
mod candidates;
mod daily;
mod metadata;
mod profile;
mod radio;
mod ranking;
mod selection;

#[cfg(test)]
mod tests;

pub(crate) use auto_continue::{generate_auto_continue, AutoContinueGenerationContext};
pub(crate) use daily::{
    current_day_key, generate_daily_mix, DailyMixGenerationContext, DailyMixSnapshot,
};
pub use profile::ListeningProfile;
pub(crate) use radio::{generate_radio, RadioGenerationContext, RadioSeed};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecommendationKind {
    DailyMix,
    SongRadio,
    ArtistRadio,
    AutoContinue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CandidateSource {
    Favorite,
    Frequent,
    Similar,
    ArtistTop,
    SameArtist,
    SameGenre,
    Newest,
    Random,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecommendationReason {
    Favorite,
    FrequentlyPlayed,
    SimilarTrack,
    PopularByArtist,
    SameArtist,
    SameGenre,
    NewRelease,
    Explore,
}

#[derive(Clone, Debug)]
pub struct RecommendationRequest {
    pub kind: RecommendationKind,
    pub target_count: usize,
    /// Stable seed, e.g. hash(server id + date + algorithm version + variant).
    pub seed: u64,
}

#[derive(Clone, Debug)]
pub struct RecommendationCandidate {
    pub song: Song,
    pub source: CandidateSource,
}

#[derive(Clone, Debug)]
pub struct RecommendationGenerationContext {
    pub server_id: String,
    pub sonic_similarity: CapabilitySupport,
    pub recent_song_ids: HashSet<String>,
    pub listening_profile: ListeningProfile,
    pub excluded_song_ids: HashSet<String>,
}

impl RecommendationGenerationContext {
    fn into_ranking_context(
        self,
        max_tracks_per_artist: usize,
        max_tracks_per_album: usize,
    ) -> RecommendationContext {
        RecommendationContext {
            excluded_song_ids: self.excluded_song_ids,
            recent_song_ids: self.recent_song_ids,
            listening_profile: self.listening_profile,
            max_tracks_per_artist,
            max_tracks_per_album,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RecommendationContext {
    /// Current queue entries are always excluded; automatic results may never
    /// duplicate or overwrite the user's explicitly arranged Up Next.
    pub excluded_song_ids: HashSet<String>,
    /// Recent tracks are deferred until non-recent candidates are exhausted.
    pub recent_song_ids: HashSet<String>,
    /// Affinity derived from actual local listening duration and completion.
    pub listening_profile: ListeningProfile,
    pub max_tracks_per_artist: usize,
    pub max_tracks_per_album: usize,
}

impl RecommendationContext {
    pub fn with_daily_mix_defaults() -> Self {
        Self {
            max_tracks_per_artist: 3,
            max_tracks_per_album: 2,
            ..Self::default()
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecommendedTrack {
    pub song: Song,
    pub reason: RecommendationReason,
    pub score: f32,
    /// Queue provenance is part of the recommendation result so every caller
    /// preserves the distinction between manual and generated tracks.
    #[serde(default)]
    pub origin: QueueOrigin,
}

/// Rank and select candidates. Selection gives all fresh candidates a strict
/// diversity pass followed by a relaxed pass before admitting recent tracks.
pub fn recommend(
    request: &RecommendationRequest,
    candidates: Vec<RecommendationCandidate>,
    context: &RecommendationContext,
) -> Vec<RecommendedTrack> {
    if request.target_count == 0 {
        return Vec::new();
    }
    let ranked = ranking::rank(request, candidates, context);
    selection::select(request, &ranked, context)
        .into_iter()
        .map(|candidate| RecommendedTrack {
            song: candidate.song.clone(),
            reason: candidate.reason,
            score: candidate.score,
            origin: recommendation_origin(request.kind),
        })
        .collect()
}

fn recommendation_origin(kind: RecommendationKind) -> QueueOrigin {
    match kind {
        RecommendationKind::DailyMix => QueueOrigin::DailyMix,
        RecommendationKind::SongRadio => QueueOrigin::SongRadio,
        RecommendationKind::ArtistRadio => QueueOrigin::ArtistRadio,
        RecommendationKind::AutoContinue => QueueOrigin::AutoContinue,
    }
}
