//! Automatic queue continuation built on the shared recommendation ranker.
//! Generation starts only after the app observes a natural sequential queue
//! boundary; this module only gathers bounded provider facts and ranks them.

use std::collections::HashSet;

use anyhow::{anyhow, Result};

use super::candidates::{
    collect_album_tracks, collect_genre_tracks, collect_similar_tracks, stable_generation_seed,
    MAX_CANDIDATES,
};
use super::{
    recommend, CandidateSource, ListeningProfile, RecommendationCandidate, RecommendationContext,
    RecommendationKind, RecommendationRequest, RecommendedTrack,
};
use crate::api::OpenSubsonicProvider;
use crate::models::{CapabilitySupport, Song};

const AUTO_CONTINUE_TARGET: usize = 20;

#[derive(Clone, Debug)]
pub struct AutoContinueGenerationContext {
    pub server_id: String,
    /// Latest song first; at most the last few queue tracks are needed.
    pub seeds: Vec<Song>,
    pub sonic_similarity: CapabilitySupport,
    pub recent_song_ids: HashSet<String>,
    pub listening_profile: ListeningProfile,
    pub excluded_song_ids: HashSet<String>,
}

pub async fn generate_auto_continue(
    api: OpenSubsonicProvider,
    context: AutoContinueGenerationContext,
) -> Result<Vec<RecommendedTrack>> {
    if context.seeds.is_empty() {
        return Err(anyhow!("automatic continuation has no queue context"));
    }

    let favorite_api = api.clone();
    let favorites = async move {
        favorite_api
            .favorites()
            .await
            .map_err(|error| format!("favorites: {error:#}"))
    };
    let random_api = api.clone();
    let random_albums = async move {
        random_api
            .albums_random(2)
            .await
            .map_err(|error| format!("random albums: {error:#}"))
    };
    let artist_api = api.clone();
    let artist_id = context
        .seeds
        .first()
        .and_then(|song| song.artist_id.clone());
    let artist_albums = async move {
        if let Some(artist_id) = artist_id.filter(|id| !id.trim().is_empty()) {
            artist_api
                .artist_albums(&artist_id)
                .await
                .map_err(|error| format!("artist albums: {error:#}"))
        } else {
            Ok(Vec::new())
        }
    };

    let (favorites, random_albums, artist_albums) =
        tokio::join!(favorites, random_albums, artist_albums);
    let mut errors = Vec::new();
    let mut candidates = favorites
        .unwrap_or_else(|error| {
            errors.push(error);
            Default::default()
        })
        .songs
        .into_iter()
        .take(40)
        .map(|song| RecommendationCandidate {
            song,
            source: CandidateSource::Favorite,
        })
        .collect::<Vec<_>>();

    let random_albums = random_albums.unwrap_or_else(|error| {
        errors.push(error);
        Vec::new()
    });
    candidates.extend(
        collect_album_tracks(api.clone(), random_albums)
            .await
            .into_iter()
            .map(|song| RecommendationCandidate {
                song,
                source: CandidateSource::Random,
            }),
    );
    let artist_albums = artist_albums.unwrap_or_else(|error| {
        errors.push(error);
        Vec::new()
    });
    candidates.extend(
        collect_album_tracks(api.clone(), artist_albums)
            .await
            .into_iter()
            .map(|song| RecommendationCandidate {
                song,
                source: CandidateSource::SameArtist,
            }),
    );
    if context.sonic_similarity.allows_optimistic_request() {
        candidates.extend(
            collect_similar_tracks(api.clone(), &context.seeds, &mut errors)
                .await
                .into_iter()
                .map(|song| RecommendationCandidate {
                    song,
                    source: CandidateSource::Similar,
                }),
        );
    }
    candidates.extend(
        collect_genre_tracks(api, &context.seeds, &mut errors)
            .await
            .into_iter()
            .map(|song| RecommendationCandidate {
                song,
                source: CandidateSource::SameGenre,
            }),
    );

    if candidates.is_empty() {
        return Err(if errors.is_empty() {
            anyhow!("the server returned no automatic continuation candidates")
        } else {
            anyhow!(errors.join("; "))
        });
    }
    if !errors.is_empty() {
        log::debug!(
            "automatic continuation generated with partial source failures: {}",
            errors.join("; ")
        );
    }
    candidates.truncate(MAX_CANDIDATES);

    let key = context
        .seeds
        .iter()
        .map(|song| song.id.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let tracks = recommend(
        &RecommendationRequest {
            kind: RecommendationKind::AutoContinue,
            target_count: AUTO_CONTINUE_TARGET,
            seed: stable_generation_seed(&context.server_id, &format!("auto:{key}")),
        },
        candidates,
        &RecommendationContext {
            excluded_song_ids: context.excluded_song_ids,
            recent_song_ids: context.recent_song_ids,
            listening_profile: context.listening_profile,
            max_tracks_per_artist: 3,
            max_tracks_per_album: 2,
        },
    );
    if tracks.is_empty() {
        Err(anyhow!(
            "all automatic continuation candidates were filtered out"
        ))
    } else {
        Ok(tracks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::QueueOrigin;

    #[test]
    fn auto_continue_prefers_queue_related_candidates_over_global_favorites() {
        let song = |id: &str| Song {
            id: id.into(),
            title: id.into(),
            artist: id.into(),
            ..Song::default()
        };
        let tracks = recommend(
            &RecommendationRequest {
                kind: RecommendationKind::AutoContinue,
                target_count: 1,
                seed: 11,
            },
            vec![
                RecommendationCandidate {
                    song: song("favorite"),
                    source: CandidateSource::Favorite,
                },
                RecommendationCandidate {
                    song: song("related"),
                    source: CandidateSource::Similar,
                },
            ],
            &RecommendationContext::default(),
        );
        assert_eq!(tracks[0].song.id, "related");
    }

    #[test]
    fn auto_continue_ranking_attaches_its_queue_origin() {
        let tracks = recommend(
            &RecommendationRequest {
                kind: RecommendationKind::AutoContinue,
                target_count: 1,
                seed: 7,
            },
            vec![RecommendationCandidate {
                song: Song {
                    id: "next".into(),
                    title: "Next".into(),
                    ..Song::default()
                },
                source: CandidateSource::Similar,
            }],
            &RecommendationContext::default(),
        );
        assert_eq!(tracks[0].origin, QueueOrigin::AutoContinue);
    }
}
