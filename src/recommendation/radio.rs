//! Candidate collection for song and artist radio. The provider only returns
//! factual lists; selection, history filtering and diversity stay in the pure
//! recommendation ranker.

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
use crate::models::{Artist, CapabilitySupport, Song};

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
    pub server_id: String,
    pub seed: RadioSeed,
    pub sonic_similarity: CapabilitySupport,
    pub top_songs_by_artist_id: CapabilitySupport,
    pub recent_song_ids: HashSet<String>,
    pub listening_profile: ListeningProfile,
    pub excluded_song_ids: HashSet<String>,
}

pub async fn generate_radio(
    api: OpenSubsonicProvider,
    context: RadioGenerationContext,
) -> Result<Vec<RecommendedTrack>> {
    let key = context.seed.key();
    let kind = context.seed.kind();
    let mut errors = Vec::new();
    let mut candidates = match &context.seed {
        RadioSeed::Song(song) => {
            collect_song_candidates(api, song, context.sonic_similarity, &mut errors).await
        }
        RadioSeed::Artist(artist) => {
            collect_artist_candidates(
                api,
                artist,
                context.sonic_similarity,
                context.top_songs_by_artist_id,
                &mut errors,
            )
            .await
        }
    };

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
            seed: stable_generation_seed(&context.server_id, &key),
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
        Err(anyhow!("all radio candidates were filtered out"))
    } else {
        Ok(tracks)
    }
}

async fn collect_song_candidates(
    api: OpenSubsonicProvider,
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
    api: OpenSubsonicProvider,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::QueueOrigin;

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
}
