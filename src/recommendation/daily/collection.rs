//! Bounded Daily Mix fact requests and candidate source attribution.

use tokio::task::JoinSet;

use super::super::candidates::genre_seeds;
use super::super::{CandidateSource, RecommendationCandidate};
use super::album_plan::AlbumRequest;
use crate::models::Song;
use crate::source::MusicSource;

const MAX_ALBUM_TRACKS: usize = 12;
const MAX_GENRE_SEEDS: usize = 2;
const REQUEST_CONCURRENCY: usize = 4;

#[cfg(test)]
mod tests;

pub(super) async fn collect_album_candidates(
    api: MusicSource,
    plan: Vec<AlbumRequest>,
) -> Vec<RecommendationCandidate> {
    let mut candidates = Vec::new();
    for chunk in plan.chunks(REQUEST_CONCURRENCY) {
        let mut tasks = JoinSet::new();
        for request in chunk {
            let api = api.clone();
            let album_id = request.id.clone();
            let sources = request.sources.clone();
            tasks.spawn(async move {
                let result = api.album_songs(&album_id).await;
                (album_id, sources, result)
            });
        }
        while let Some(result) = tasks.join_next().await {
            match result {
                Ok((_album_id, sources, Ok(songs))) => {
                    for song in songs.into_iter().take(MAX_ALBUM_TRACKS) {
                        for source in &sources {
                            candidates.push(RecommendationCandidate {
                                song: song.clone(),
                                source: *source,
                            });
                        }
                    }
                }
                Ok((album_id, _, Err(error))) => {
                    log::debug!("daily mix album {album_id} unavailable: {error:#}")
                }
                Err(error) => log::debug!("daily mix album task failed: {error}"),
            }
        }
    }
    candidates
}

pub(super) async fn collect_similar_candidates(
    api: MusicSource,
    seeds: &[Song],
) -> Vec<RecommendationCandidate> {
    let mut candidates = Vec::new();
    for chunk in seeds.chunks(REQUEST_CONCURRENCY) {
        let mut tasks = JoinSet::new();
        for song in chunk {
            let api = api.clone();
            let song_id = song.id.clone();
            tasks.spawn(async move { (song_id.clone(), api.similar_songs(&song_id, 20).await) });
        }
        while let Some(result) = tasks.join_next().await {
            match result {
                Ok((_seed, Ok(songs))) => {
                    candidates.extend(songs.into_iter().map(|song| RecommendationCandidate {
                        song,
                        source: CandidateSource::Similar,
                    }))
                }
                Ok((seed, Err(error))) => {
                    log::debug!("daily mix similarity seed {seed} unavailable: {error:#}")
                }
                Err(error) => log::debug!("daily mix similarity task failed: {error}"),
            }
        }
    }
    candidates
}

pub(super) async fn collect_genre_candidates(
    api: MusicSource,
    seeds: &[Song],
) -> Vec<RecommendationCandidate> {
    let genres = genre_seeds(seeds, MAX_GENRE_SEEDS);
    let mut tasks = JoinSet::new();
    for genre in genres {
        let api = api.clone();
        tasks.spawn(async move {
            let result = api.songs_by_genre(&genre, 20, 0).await;
            (genre, result)
        });
    }
    let mut candidates = Vec::new();
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok((_genre, Ok(songs))) => {
                candidates.extend(songs.into_iter().map(|song| RecommendationCandidate {
                    song,
                    source: CandidateSource::SameGenre,
                }))
            }
            Ok((genre, Err(error))) => {
                log::debug!("daily mix genre {genre:?} unavailable: {error:#}")
            }
            Err(error) => log::debug!("daily mix genre task failed: {error}"),
        }
    }
    candidates
}
