//! In-memory and persisted local library index built from scanned audio files.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use super::favorites::LocalFavoritesStore;
use crate::models::{Album, AlbumSort, Artist, Favorites, Genre, SearchResults, Song};

const INDEX_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) enum CoverOrigin {
    ImageFile(PathBuf),
    EmbeddedAudio(PathBuf),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct IndexedTrack {
    pub(super) file_path: PathBuf,
    pub(super) mtime_secs: u64,
    pub(super) file_size: u64,
    pub(super) year: Option<i32>,
    pub(super) album_artist: String,
    pub(super) cover_origin: Option<CoverOrigin>,
    pub(super) song: Song,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PersistedIndex {
    version: u32,
    tracks: Vec<IndexedTrack>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct LocalLibraryIndex {
    tracks_by_path: HashMap<PathBuf, IndexedTrack>,
    tracks_by_id: HashMap<String, usize>,
    tracks: Vec<IndexedTrack>,
    artists: Vec<Artist>,
    albums: Vec<Album>,
    album_mtime: HashMap<String, u64>,
    album_tracks: HashMap<String, Vec<usize>>,
    artist_albums: HashMap<String, Vec<String>>,
    artist_tracks: HashMap<String, Vec<usize>>,
    genres: Vec<Genre>,
    covers: HashMap<String, CoverOrigin>,
}

impl LocalLibraryIndex {
    pub(super) fn load(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        let Ok(persisted) = serde_json::from_str::<PersistedIndex>(&text) else {
            return Self::default();
        };
        if persisted.version != INDEX_SCHEMA_VERSION {
            return Self::default();
        }
        Self::from_tracks(persisted.tracks)
    }

    pub(super) fn save(&self, path: &Path) -> Result<()> {
        let persisted = PersistedIndex {
            version: INDEX_SCHEMA_VERSION,
            tracks: self.tracks.clone(),
        };
        crate::config::write_json_atomic(path, &persisted, "local library index")
    }

    pub(super) fn existing_track(&self, path: &Path) -> Option<&IndexedTrack> {
        self.tracks_by_path.get(path)
    }

    pub(super) fn track_count(&self) -> usize {
        self.tracks.len()
    }

    pub(super) fn song_path(&self, song_id: &str) -> Option<&Path> {
        let &idx = self.tracks_by_id.get(song_id)?;
        Some(self.tracks[idx].file_path.as_path())
    }

    pub(super) fn cover_origin(&self, cover_id: &str) -> Option<&CoverOrigin> {
        self.covers.get(cover_id)
    }

    pub(super) fn from_tracks(mut tracks: Vec<IndexedTrack>) -> Self {
        tracks.sort_by(|a, b| a.file_path.cmp(&b.file_path));

        let mut tracks_by_path = HashMap::with_capacity(tracks.len());
        let mut tracks_by_id = HashMap::with_capacity(tracks.len());
        let mut covers: HashMap<String, CoverOrigin> = HashMap::new();

        // Group tracks by artist_id and album_id.
        struct ArtistAgg {
            id: String,
            name: String,
            album_ids: Vec<String>,
            track_indices: Vec<usize>,
            cover_art: Option<String>,
        }
        struct AlbumAgg {
            id: String,
            name: String,
            artist: String,
            artist_id: String,
            cover_art: Option<String>,
            track_indices: Vec<usize>,
            duration: i32,
            year: Option<i32>,
            genre: Option<String>,
            latest_mtime: u64,
        }

        let mut artist_map: HashMap<String, ArtistAgg> = HashMap::new();
        let mut album_map: HashMap<String, AlbumAgg> = HashMap::new();
        let mut genre_songs: BTreeMap<String, (i64, std::collections::HashSet<String>)> =
            BTreeMap::new();

        for (idx, track) in tracks.iter().enumerate() {
            tracks_by_path.insert(track.file_path.clone(), track.clone());
            tracks_by_id.insert(track.song.id.clone(), idx);
            if let (Some(cover_id), Some(origin)) = (&track.song.cover_art, &track.cover_origin) {
                covers
                    .entry(cover_id.clone())
                    .or_insert_with(|| origin.clone());
            }

            let artist_name = if !track.album_artist.trim().is_empty() {
                track.album_artist.trim()
            } else if !track.song.artist.trim().is_empty() {
                track.song.artist.trim()
            } else {
                "Unknown Artist"
            };
            let artist_id = track
                .song
                .artist_id
                .clone()
                .unwrap_or_else(|| stable_id("artist", artist_name));
            let album_name = if track.song.album.trim().is_empty() {
                "Unknown Album"
            } else {
                track.song.album.trim()
            };
            let album_id = track
                .song
                .album_id
                .clone()
                .unwrap_or_else(|| stable_id("album", &format!("{artist_id}\0{album_name}")));

            let artist_entry = artist_map
                .entry(artist_id.clone())
                .or_insert_with(|| ArtistAgg {
                    id: artist_id.clone(),
                    name: artist_name.to_string(),
                    album_ids: Vec::new(),
                    track_indices: Vec::new(),
                    cover_art: None,
                });
            artist_entry.track_indices.push(idx);
            if artist_entry.cover_art.is_none() {
                artist_entry.cover_art.clone_from(&track.song.cover_art);
            }
            if !artist_entry.album_ids.contains(&album_id) {
                artist_entry.album_ids.push(album_id.clone());
            }

            let album_entry = album_map
                .entry(album_id.clone())
                .or_insert_with(|| AlbumAgg {
                    id: album_id.clone(),
                    name: album_name.to_string(),
                    artist: artist_name.to_string(),
                    artist_id: artist_id.clone(),
                    cover_art: None,
                    track_indices: Vec::new(),
                    duration: 0,
                    year: None,
                    genre: None,
                    latest_mtime: 0,
                });
            album_entry.track_indices.push(idx);
            album_entry.duration = album_entry
                .duration
                .saturating_add(track.song.duration.unwrap_or(0).max(0));
            if album_entry.cover_art.is_none() {
                album_entry.cover_art.clone_from(&track.song.cover_art);
            }
            if album_entry.year.is_none() {
                album_entry.year = track.year;
            }
            if album_entry.genre.is_none() {
                album_entry.genre.clone_from(&track.song.genre);
            }
            album_entry.latest_mtime = album_entry.latest_mtime.max(track.mtime_secs);

            if let Some(genre) = track
                .song
                .genre
                .as_deref()
                .map(str::trim)
                .filter(|g| !g.is_empty())
            {
                let entry = genre_songs
                    .entry(genre.to_string())
                    .or_insert_with(|| (0, std::collections::HashSet::new()));
                entry.0 += 1;
                entry.1.insert(album_id);
            }
        }

        let mut artists: Vec<Artist> = Vec::with_capacity(artist_map.len());
        let mut artist_albums_map: HashMap<String, Vec<String>> = HashMap::new();
        let mut artist_tracks_map: HashMap<String, Vec<usize>> = HashMap::new();
        for (_, agg) in artist_map {
            artists.push(Artist {
                id: agg.id.clone(),
                name: agg.name,
                album_count: i32::try_from(agg.album_ids.len()).ok(),
                cover_art: agg.cover_art,
            });
            artist_albums_map.insert(agg.id.clone(), agg.album_ids);
            artist_tracks_map.insert(agg.id, agg.track_indices);
        }
        artists.sort_by_key(|a| a.name.to_lowercase());

        let mut albums: Vec<Album> = Vec::with_capacity(album_map.len());
        let mut album_mtime = HashMap::with_capacity(album_map.len());
        let mut album_tracks_map: HashMap<String, Vec<usize>> =
            HashMap::with_capacity(album_map.len());
        for (_, mut agg) in album_map {
            agg.track_indices.sort_by(|&a, &b| {
                let sa = &tracks[a].song;
                let sb = &tracks[b].song;
                (
                    sa.disc.unwrap_or(1),
                    sa.track.unwrap_or(i32::MAX),
                    sa.title.to_lowercase(),
                )
                    .cmp(&(
                        sb.disc.unwrap_or(1),
                        sb.track.unwrap_or(i32::MAX),
                        sb.title.to_lowercase(),
                    ))
            });
            let created = chrono::DateTime::from_timestamp(agg.latest_mtime as i64, 0)
                .map(|dt| dt.to_rfc3339());
            albums.push(Album {
                id: agg.id.clone(),
                name: agg.name,
                artist: agg.artist,
                artist_id: Some(agg.artist_id),
                cover_art: agg.cover_art,
                song_count: i32::try_from(agg.track_indices.len()).ok(),
                duration: Some(agg.duration),
                year: agg.year,
                genre: agg.genre,
                created,
            });
            album_mtime.insert(agg.id.clone(), agg.latest_mtime);
            album_tracks_map.insert(agg.id, agg.track_indices);
        }
        albums.sort_by_key(|a| a.name.to_lowercase());

        let genres = genre_songs
            .into_iter()
            .map(|(value, (songs, album_set))| Genre {
                value,
                song_count: Some(songs),
                album_count: Some(album_set.len() as i64),
            })
            .collect();

        Self {
            tracks_by_path,
            tracks_by_id,
            tracks,
            artists,
            albums,
            album_mtime,
            album_tracks: album_tracks_map,
            artist_albums: artist_albums_map,
            artist_tracks: artist_tracks_map,
            genres,
            covers,
        }
    }

    pub(super) fn artists(&self) -> Vec<Artist> {
        self.artists.clone()
    }

    pub(super) fn artist_albums(&self, artist_id: &str) -> Vec<Album> {
        let Some(ids) = self.artist_albums.get(artist_id) else {
            return Vec::new();
        };
        let mut albums: Vec<Album> = self
            .albums
            .iter()
            .filter(|album| ids.contains(&album.id))
            .cloned()
            .collect();
        albums.sort_by(|a, b| {
            b.year
                .unwrap_or(0)
                .cmp(&a.year.unwrap_or(0))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        albums
    }

    pub(super) fn artist_songs(&self, artist_id: &str, artist_name: &str, count: usize) -> Vec<Song> {
        if let Some(indices) = self.artist_tracks.get(artist_id) {
            return indices
                .iter()
                .take(count)
                .map(|&idx| self.tracks[idx].song.clone())
                .collect();
        }
        let needle = artist_name.to_lowercase();
        self.tracks
            .iter()
            .filter(|track| track.song.artist.to_lowercase() == needle)
            .take(count)
            .map(|track| track.song.clone())
            .collect()
    }

    pub(super) fn all_albums(&self) -> Vec<Album> {
        self.albums.clone()
    }

    pub(super) fn albums(&self, size: usize, offset: usize, sort: AlbumSort) -> Vec<Album> {
        let mut albums = self.albums.clone();
        match sort {
            AlbumSort::NewestAdded => {
                albums.sort_by(|a, b| {
                    let ma = self.album_mtime.get(&a.id).copied().unwrap_or(0);
                    let mb = self.album_mtime.get(&b.id).copied().unwrap_or(0);
                    mb.cmp(&ma)
                        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });
            }
            AlbumSort::Name => {
                albums.sort_by_key(|a| a.name.to_lowercase());
            }
            AlbumSort::Artist => {
                albums.sort_by(|a, b| {
                    a.artist
                        .to_lowercase()
                        .cmp(&b.artist.to_lowercase())
                        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });
            }
            AlbumSort::Year => {
                albums.sort_by(|a, b| {
                    b.year
                        .unwrap_or(0)
                        .cmp(&a.year.unwrap_or(0))
                        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });
            }
        }
        albums.into_iter().skip(offset).take(size).collect()
    }

    pub(super) fn genres(&self) -> Vec<Genre> {
        self.genres.clone()
    }

    pub(super) fn albums_by_genre(&self, genre: &str, size: usize, offset: usize) -> Vec<Album> {
        let needle = genre.trim().to_lowercase();
        self.albums
            .iter()
            .filter(|album| {
                album
                    .genre
                    .as_deref()
                    .is_some_and(|g| g.trim().to_lowercase() == needle)
            })
            .skip(offset)
            .take(size)
            .cloned()
            .collect()
    }

    pub(super) fn album_songs(&self, album_id: &str) -> Vec<Song> {
        self.album_tracks
            .get(album_id)
            .map(|indices| {
                indices
                    .iter()
                    .map(|&idx| self.tracks[idx].song.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(super) fn songs_by_genre(&self, genre: &str, count: usize, offset: usize) -> Vec<Song> {
        let needle = genre.trim().to_lowercase();
        self.tracks
            .iter()
            .filter(|track| {
                track
                    .song
                    .genre
                    .as_deref()
                    .is_some_and(|g| g.trim().to_lowercase() == needle)
            })
            .skip(offset)
            .take(count)
            .map(|track| track.song.clone())
            .collect()
    }

    pub(super) fn resolve_favorites(&self, store: &LocalFavoritesStore) -> Favorites {
        let artists = self
            .artists
            .iter()
            .filter(|artist| store.artists.contains(&artist.id))
            .cloned()
            .collect();
        let albums = self
            .albums
            .iter()
            .filter(|album| store.albums.contains(&album.id))
            .cloned()
            .collect();
        let songs = self
            .tracks
            .iter()
            .filter(|track| store.songs.contains(&track.song.id))
            .map(|track| track.song.clone())
            .collect();
        Favorites {
            artists,
            albums,
            songs,
        }
    }

    pub(super) fn search(
        &self,
        query: &str,
        song_limit: usize,
        album_limit: usize,
        artist_limit: usize,
    ) -> SearchResults {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return SearchResults::default();
        }
        let artists = self
            .artists
            .iter()
            .filter(|artist| artist.name.to_lowercase().contains(&needle))
            .take(artist_limit)
            .cloned()
            .collect();
        let albums = self
            .albums
            .iter()
            .filter(|album| {
                album.name.to_lowercase().contains(&needle)
                    || album.artist.to_lowercase().contains(&needle)
            })
            .take(album_limit)
            .cloned()
            .collect();
        let songs = self
            .tracks
            .iter()
            .filter(|track| {
                track.song.title.to_lowercase().contains(&needle)
                    || track.song.artist.to_lowercase().contains(&needle)
                    || track.song.album.to_lowercase().contains(&needle)
            })
            .take(song_limit)
            .map(|track| track.song.clone())
            .collect();
        SearchResults {
            artists,
            albums,
            songs,
        }
    }
}

pub(super) fn stable_id(prefix: &str, key: &str) -> String {
    let mut hash = Md5::new();
    hash.update(prefix.as_bytes());
    hash.update([0]);
    hash.update(key.as_bytes());
    format!("{prefix}-{:016x}", u64::from_be_bytes(hash.finalize()[..8].try_into().unwrap()))
}
