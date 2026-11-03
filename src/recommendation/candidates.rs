//! Shared, bounded provider fact collection for recommendation requests.
//! This module intentionally does not score or select tracks; callers attach
//! candidate sources and pass the facts to the pure ranker.

use md5::{Digest, Md5};
use tokio::task::JoinSet;

use crate::api::OpenSubsonicProvider;
use crate::models::{Album, Song};

pub(super) const MAX_CANDIDATES: usize = 180;
const MAX_ALBUMS: usize = 8;
const MAX_TRACKS_PER_ALBUM: usize = 12;
const MAX_SIMILARITY_SEEDS: usize = 3;
const MAX_GENRE_SEEDS: usize = 2;
const REQUEST_CONCURRENCY: usize = 4;

pub(super) async fn collect_album_tracks(
    api: OpenSubsonicProvider,
    albums: Vec<Album>,
) -> Vec<Song> {
    let mut songs = Vec::new();
    let albums = albums.into_iter().take(MAX_ALBUMS).collect::<Vec<_>>();
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
                    log::debug!("recommendation album {album_id} unavailable: {error:#}");
                }
                Err(error) => log::debug!("recommendation album task failed: {error}"),
            }
        }
    }
    songs
}

pub(super) async fn collect_similar_tracks(
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

pub(super) async fn collect_genre_tracks(
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

pub(super) fn stable_generation_seed(server_id: &str, key: &str) -> u64 {
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

    #[test]
    fn generation_seed_is_stable_and_source_scoped() {
        assert_eq!(
            stable_generation_seed("server", "song:a"),
            stable_generation_seed("server", "song:a")
        );
        assert_ne!(
            stable_generation_seed("server", "song:a"),
            stable_generation_seed("other", "song:a")
        );
        assert_ne!(
            stable_generation_seed("server", "song:a"),
            stable_generation_seed("server", "artist:a")
        );
    }
}
