//! Generation orchestration for song and artist radio. The provider only returns
//! factual lists; selection, history filtering and diversity stay in the pure
//! recommendation ranker.

use anyhow::{anyhow, Result};

use super::candidates::{stable_generation_seed, MAX_CANDIDATES};
use super::{
    recommend, RecommendationCandidate, RecommendationGenerationContext, RecommendationKind,
    RecommendationRequest, RecommendedTrack,
};
use crate::api::OpenSubsonicProvider;
use crate::models::{Artist, CapabilitySupport, Song};

mod collection;

const RADIO_TARGET: usize = 24;

#[derive(Clone, Debug)]
pub enum RadioSeed {
    Song(Box<Song>),
    Artist(Artist),
}

impl RadioSeed {
    pub fn key(&self) -> String {
        match self {
            Self::Song(song) => format!("song:{}", song.id),
            Self::Artist(artist) => format!("artist:{}", artist.id),
        }
    }

    fn kind(&self) -> RecommendationKind {
        match self {
            Self::Song(_) => RecommendationKind::SongRadio,
            Self::Artist(_) => RecommendationKind::ArtistRadio,
        }
    }
}

#[derive(Clone, Debug)]
pub struct RadioGenerationContext {
    pub common: RecommendationGenerationContext,
    pub seed: RadioSeed,
    pub top_songs_by_artist_id: CapabilitySupport,
}

pub async fn generate_radio(
    api: OpenSubsonicProvider,
    context: RadioGenerationContext,
) -> Result<Vec<RecommendedTrack>> {
    let (candidates, errors) = collection::collect(
        api,
        &context.seed,
        context.common.sonic_similarity,
        context.top_songs_by_artist_id,
    )
    .await;
    context.select(candidates, errors)
}

impl RadioGenerationContext {
    /// Resolve collection failures and build a bounded ranking request without
    /// issuing provider requests.
    fn select(
        self,
        mut candidates: Vec<RecommendationCandidate>,
        errors: Vec<String>,
    ) -> Result<Vec<RecommendedTrack>> {
        let key = self.seed.key();
        let kind = self.seed.kind();
        let common = self.common;
        if candidates.is_empty() {
            return Err(if errors.is_empty() {
                anyhow!("the server returned no radio candidates")
            } else {
                anyhow!(errors.join("; "))
            });
        }
        if !errors.is_empty() {
            log::debug!(
                "radio generated with partial source failures: {}",
                errors.join("; ")
            );
        }
        candidates.truncate(MAX_CANDIDATES);

        let tracks = recommend(
            &RecommendationRequest {
                kind,
                target_count: RADIO_TARGET,
                seed: stable_generation_seed(&common.server_id, &key),
            },
            candidates,
            &common.into_ranking_context(3, 2),
        );
        if tracks.is_empty() {
            Err(anyhow!("all radio candidates were filtered out"))
        } else {
            Ok(tracks)
        }
    }
}

#[cfg(test)]
mod tests;
