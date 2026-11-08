//! Daily Mix orchestration with separate cache, style and request policies.

use anyhow::{anyhow, Result};

mod album_plan;
mod collection;
mod snapshot;
mod style;
#[cfg(test)]
mod tests;

use super::{
    recommend, CandidateSource, RecommendationCandidate, RecommendationContext,
    RecommendationGenerationContext, RecommendationKind, RecommendationRequest,
};
use crate::source::MusicSource;
use album_plan::merge_album_plan;
use collection::{collect_album_candidates, collect_genre_candidates, collect_similar_candidates};
use snapshot::daily_seed;
pub use snapshot::{current_day_key, DailyMixSnapshot, DAILY_MIX_VERSION};
use style::{select_style_seeds, StyleSignature};

const DAILY_TARGET: usize = 30;
const MAX_CANDIDATES: usize = 240;

#[derive(Clone, Debug)]
pub struct DailyMixGenerationContext {
    pub common: RecommendationGenerationContext,
    pub day: String,
    pub variant: u32,
}

pub async fn generate_daily_mix(
    api: MusicSource,
    context: DailyMixGenerationContext,
) -> Result<DailyMixSnapshot> {
    let DailyMixGenerationContext {
        common,
        day,
        variant,
    } = context;
    let RecommendationGenerationContext {
        server_id,
        sonic_similarity,
        recent_song_ids,
        listening_profile,
        excluded_song_ids,
    } = common;
    // Independent factual sources fail softly. A temporary failure in one
    // shelf must not discard usable candidates from the others.
    let (favorites, frequent, recent, newest, random) = tokio::join!(
        api.favorites(),
        api.albums_frequent(4),
        api.albums_recent(2),
        api.albums_newest(3),
        api.albums_random(3),
    );
    let mut source_errors = Vec::new();
    let favorites = favorites.unwrap_or_else(|error| {
        source_errors.push(format!("favorites: {error:#}"));
        Default::default()
    });
    let frequent = frequent.unwrap_or_else(|error| {
        source_errors.push(format!("frequent albums: {error:#}"));
        Vec::new()
    });
    let newest = newest.unwrap_or_else(|error| {
        source_errors.push(format!("newest albums: {error:#}"));
        Vec::new()
    });
    let recent = recent.unwrap_or_else(|error| {
        source_errors.push(format!("recent albums: {error:#}"));
        Vec::new()
    });
    let random = random.unwrap_or_else(|error| {
        source_errors.push(format!("random albums: {error:#}"));
        Vec::new()
    });

    let mut candidates = favorites
        .songs
        .iter()
        .take(60)
        .cloned()
        .map(|song| RecommendationCandidate {
            song,
            source: CandidateSource::Favorite,
        })
        .collect::<Vec<_>>();

    let album_plan = merge_album_plan([
        (frequent, CandidateSource::Frequent),
        (recent, CandidateSource::Frequent),
        (newest, CandidateSource::Newest),
        (random, CandidateSource::Random),
    ]);
    let album_candidates = collect_album_candidates(api.clone(), album_plan).await;
    candidates.extend(album_candidates);

    let seeds = select_style_seeds(&favorites.songs, &candidates, &listening_profile);
    let style = StyleSignature::from_seeds(&seeds);
    candidates.retain(|candidate| {
        !matches!(
            candidate.source,
            CandidateSource::Newest | CandidateSource::Random
        ) || style.matches(&candidate.song, &listening_profile)
    });

    if sonic_similarity.allows_optimistic_request() {
        candidates.extend(collect_similar_candidates(api.clone(), &seeds).await);
    }
    candidates.extend(collect_genre_candidates(api, &seeds).await);

    if candidates.is_empty() {
        return Err(if source_errors.is_empty() {
            anyhow!("the server returned no daily mix candidates")
        } else {
            anyhow!(source_errors.join("; "))
        });
    }
    if !source_errors.is_empty() {
        log::debug!(
            "daily mix generated with partial source failures: {}",
            source_errors.join("; ")
        );
    }
    candidates.truncate(MAX_CANDIDATES);

    let seed = daily_seed(&server_id, &day, DAILY_MIX_VERSION, variant);
    let tracks = recommend(
        &RecommendationRequest {
            kind: RecommendationKind::DailyMix,
            target_count: DAILY_TARGET,
            seed,
        },
        candidates,
        &RecommendationContext {
            excluded_song_ids,
            recent_song_ids,
            listening_profile,
            ..RecommendationContext::with_daily_mix_defaults()
        },
    );
    if tracks.is_empty() {
        return Err(anyhow!("all daily mix candidates were filtered out"));
    }

    Ok(DailyMixSnapshot {
        server_id,
        day,
        algorithm_version: DAILY_MIX_VERSION,
        variant,
        generated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        tracks,
    })
}
