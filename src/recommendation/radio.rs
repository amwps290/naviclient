//! Candidate collection for song and artist radio. The provider only returns
//! factual lists; selection, history filtering and diversity stay in the pure
//! recommendation ranker.

use std::collections::HashSet;

use anyhow::{anyhow, Result};
use md5::{Digest, Md5};
use tokio::task::JoinSet;

use super::{
    recommend, CandidateSource, ListeningProfile, RecommendationCandidate, RecommendationContext,
    RecommendationKind, RecommendationRequest, RecommendedTrack,
};
use crate::api::OpenSubsonicProvider;
use crate::models::{Artist, CapabilitySupport, Song};

const RADIO_TARGET: usize = 24;
const MAX_ARTIST_ALBUMS: usize = 8;
const MAX_TRACKS_PER_ALBUM: usize = 12;
const MAX_SIMILARITY_SEEDS: usize = 3;
const MAX_GENRE_SEEDS: usize = 2;
const MAX_CANDIDATES: usize = 180;
const REQUEST_CONCURRENCY: usize = 4;

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
            seed: stable_radio_seed(&context.server_id, &key),
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

async fn collect_album_tracks(
    api: OpenSubsonicProvider,
    albums: Vec<crate::models::Album>,
) -> Vec<Song> {
    let mut songs = Vec::new();
    let albums = albums
        .into_iter()
        .take(MAX_ARTIST_ALBUMS)
        .collect::<Vec<_>>();
    for chunk in albums.chunks(REQUEST_CONCURRENCY) {
        let mut tasks = JoinSet::new();
        for album in chunk {
            let api = api.clone();
            let album_id = album.id.clone();
            tasks.spawn(async move { (album_id.clone(), api.album_songs(&album_id).await) });
        }
        while let Some(result) = tasks.join_next().await {
            match result {
                Ok((_album_id, Ok(tracks))) => {
                    songs.extend(tracks.into_iter().take(MAX_TRACKS_PER_ALBUM));
                }
                Ok((album_id, Err(error))) => {
                    log::debug!("radio album {album_id} unavailable: {error:#}");
                }
                Err(error) => log::debug!("radio album task failed: {error}"),
            }
        }
    }
    songs
}

async fn collect_similar_tracks(
    api: OpenSubsonicProvider,
    seeds: &[Song],
    errors: &mut Vec<String>,
) -> Vec<Song> {
    let mut tasks = JoinSet::new();
    for seed in seeds.iter().take(MAX_SIMILARITY_SEEDS) {
        let api = api.clone();
        let id = seed.id.clone();
        tasks.spawn(async move { (id.clone(), api.similar_songs(&id, 24).await) });
    }
    let mut songs = Vec::new();
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok((_id, Ok(similar))) => songs.extend(similar),
            Ok((id, Err(error))) => errors.push(format!("similar songs for {id}: {error:#}")),
            Err(error) => errors.push(format!("similar songs task: {error}")),
        }
    }
    songs
}

async fn collect_genre_tracks(
    api: OpenSubsonicProvider,
    seeds: &[Song],
    errors: &mut Vec<String>,
) -> Vec<Song> {
    let mut genres = Vec::<String>::new();
    for genre in seeds
        .iter()
        .filter_map(|song| song.genre.as_deref())
        .map(str::trim)
        .filter(|genre| !genre.is_empty())
    {
        if !genres.iter().any(|known| known.eq_ignore_ascii_case(genre)) {
            genres.push(genre.to_string());
            if genres.len() == MAX_GENRE_SEEDS {
                break;
            }
        }
    }
    let mut tasks = JoinSet::new();
    for genre in genres {
        let api = api.clone();
        tasks.spawn(async move {
            let result = api.songs_by_genre(&genre, 30, 0).await;
            (genre, result)
        });
    }
    let mut songs = Vec::new();
    while let Some(result) = tasks.join_next().await {
        match result {
            Ok((_genre, Ok(genre_songs))) => songs.extend(genre_songs),
            Ok((genre, Err(error))) => errors.push(format!("genre {genre}: {error:#}")),
            Err(error) => errors.push(format!("genre songs task: {error}")),
        }
    }
    songs
}

fn stable_radio_seed(server_id: &str, key: &str) -> u64 {
    let mut hasher = Md5::new();
    hasher.update(server_id.as_bytes());
    hasher.update(key.as_bytes());
    let digest = hasher.finalize();
    u64::from_le_bytes([
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
    ])
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
    fn radio_seed_is_stable_and_server_scoped() {
        assert_eq!(
            stable_radio_seed("server", "song:a"),
            stable_radio_seed("server", "song:a")
        );
        assert_ne!(
            stable_radio_seed("server", "song:a"),
            stable_radio_seed("other", "song:a")
        );
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
