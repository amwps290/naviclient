//! Provider requests and source tagging for song and artist radio facts.

use super::super::candidates::{
    collect_album_tracks, collect_genre_tracks, collect_similar_tracks,
};
use super::super::{CandidateSource, RecommendationCandidate};
use super::RadioSeed;
use crate::models::{Artist, CapabilitySupport, Song};
use crate::source::MusicSource;

pub(super) async fn collect(
    api: MusicSource,
    seed: &RadioSeed,
    sonic_similarity: CapabilitySupport,
    top_songs_by_artist_id: CapabilitySupport,
) -> (Vec<RecommendationCandidate>, Vec<String>) {
    let mut errors = Vec::new();
    let candidates = match seed {
        RadioSeed::Song(song) => {
            collect_song_candidates(api, song, sonic_similarity, &mut errors).await
        }
        RadioSeed::Artist(artist) => {
            collect_artist_candidates(
                api,
                artist,
                sonic_similarity,
                top_songs_by_artist_id,
                &mut errors,
            )
            .await
        }
    };
    (candidates, errors)
}

async fn collect_song_candidates(
    api: MusicSource,
    seed: &Song,
    sonic_similarity: CapabilitySupport,
    errors: &mut Vec<String>,
) -> Vec<RecommendationCandidate> {
    let similarity_api = api.clone();
    let similarity_id = seed.id.clone();
    let similar = async move {
        if sonic_similarity.allows_optimistic_request() {
            similarity_api
                .similar_songs(&similarity_id, 48)
                .await
                .map_err(|error| format!("similar songs: {error:#}"))
        } else {
            Ok(Vec::new())
        }
    };

    let artist_api = api.clone();
    let artist_id = seed.artist_id.clone();
    let albums = async move {
        if let Some(artist_id) = artist_id.filter(|id| !id.trim().is_empty()) {
            artist_api
                .artist_albums(&artist_id)
                .await
                .map_err(|error| format!("artist albums: {error:#}"))
        } else {
            Ok(Vec::new())
        }
    };

    let genre_api = api.clone();
    let genre = seed.genre.clone();
    let genres = async move {
        if let Some(genre) = genre.filter(|genre| !genre.trim().is_empty()) {
            genre_api
                .songs_by_genre(&genre, 48, 0)
                .await
                .map_err(|error| format!("genre songs: {error:#}"))
        } else {
            Ok(Vec::new())
        }
    };

    let (similar, albums, genre_songs) = tokio::join!(similar, albums, genres);
    let mut candidates = tagged_or_record(similar, CandidateSource::Similar, errors);
    let albums = albums.unwrap_or_else(|error| {
        errors.push(error);
        Vec::new()
    });
    candidates.extend(
        collect_album_tracks(api, albums)
            .await
            .into_iter()
            .map(|song| RecommendationCandidate {
                song,
                source: CandidateSource::SameArtist,
            }),
    );
    candidates.extend(tagged_or_record(
        genre_songs,
        CandidateSource::SameGenre,
        errors,
    ));
    candidates
}

async fn collect_artist_candidates(
    api: MusicSource,
    artist: &Artist,
    sonic_similarity: CapabilitySupport,
    top_songs_by_artist_id: CapabilitySupport,
    errors: &mut Vec<String>,
) -> Vec<RecommendationCandidate> {
    let top_api = api.clone();
    let top_artist = artist.clone();
    let top = async move {
        top_api
            .artist_top_songs(&top_artist, 36, top_songs_by_artist_id)
            .await
            .map_err(|error| format!("artist top songs: {error:#}"))
    };
    let album_api = api.clone();
    let artist_id = artist.id.clone();
    let albums = async move {
        album_api
            .artist_albums(&artist_id)
            .await
            .map_err(|error| format!("artist albums: {error:#}"))
    };
    let (top, albums) = tokio::join!(top, albums);
    let top_songs = top.unwrap_or_else(|error| {
        errors.push(error);
        Vec::new()
    });
    let albums = albums.unwrap_or_else(|error| {
        errors.push(error);
        Vec::new()
    });

    let mut candidates = top_songs
        .iter()
        .cloned()
        .map(|song| RecommendationCandidate {
            song,
            source: CandidateSource::ArtistTop,
        })
        .collect::<Vec<_>>();
    candidates.extend(
        collect_album_tracks(api.clone(), albums)
            .await
            .into_iter()
            .map(|song| RecommendationCandidate {
                song,
                source: CandidateSource::SameArtist,
            }),
    );
    if sonic_similarity.allows_optimistic_request() {
        candidates.extend(
            collect_similar_tracks(api.clone(), &top_songs, errors)
                .await
                .into_iter()
                .map(|song| RecommendationCandidate {
                    song,
                    source: CandidateSource::Similar,
                }),
        );
    }
    candidates.extend(
        collect_genre_tracks(api, &top_songs, errors)
            .await
            .into_iter()
            .map(|song| RecommendationCandidate {
                song,
                source: CandidateSource::SameGenre,
            }),
    );
    candidates
}

fn tagged_or_record(
    result: std::result::Result<Vec<Song>, String>,
    source: CandidateSource,
    errors: &mut Vec<String>,
) -> Vec<RecommendationCandidate> {
    result
        .unwrap_or_else(|error| {
            errors.push(error);
            Vec::new()
        })
        .into_iter()
        .map(|song| RecommendationCandidate { song, source })
        .collect()
}
