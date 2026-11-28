//! Persistent WebDAV library index, path metadata inference, and favorites store.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;

use anyhow::Result;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use super::client::{DavEntry, PeekedAudioTags};
use crate::models::{
    Album, AlbumSort, Artist, FavoriteKey, FavoriteKind, Favorites, FolderItem, FolderListing,
    Genre, SearchResults, Song,
};
use crate::source::SourceId;

const INDEX_SCHEMA_VERSION: u32 = 1;

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "oga", "opus", "m4a", "mp4", "aac", "alac", "wav", "aif", "aiff",
];

const FOLDER_COVER_NAMES: &[&str] = &[
    "cover.jpg",
    "cover.jpeg",
    "cover.png",
    "folder.jpg",
    "folder.jpeg",
    "folder.png",
    "front.jpg",
    "front.jpeg",
    "front.png",
];

pub(super) fn is_supported_audio_name(file_name: &str) -> bool {
    file_name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .is_some_and(|ext| SUPPORTED_EXTENSIONS.contains(&ext.as_str()))
}

pub(super) fn folder_cover_rank(lower_name: &str) -> Option<usize> {
    FOLDER_COVER_NAMES.iter().position(|&name| name == lower_name)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct WebDavTrack {
    pub(super) file_url: String,
    pub(super) etag: Option<String>,
    pub(super) content_length: Option<u64>,
    pub(super) last_modified: Option<String>,
    pub(super) year: Option<i32>,
    pub(super) album_artist: String,
    pub(super) cover_url: Option<String>,
    pub(super) lrc_url: Option<String>,
    pub(super) song: Song,
}

impl WebDavTrack {
    pub(super) fn matches_remote(
        &self,
        etag: Option<&str>,
        content_length: Option<u64>,
        last_modified: Option<&str>,
    ) -> bool {
        if etag.is_some() && self.etag.as_deref() == etag {
            return true;
        }
        self.content_length == content_length
            && self.last_modified.as_deref() == last_modified
            && (content_length.is_some() || last_modified.is_some())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PersistedWebDavIndex {
    version: u32,
    tracks: Vec<WebDavTrack>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct WebDavLibraryIndex {
    tracks_by_url: HashMap<String, WebDavTrack>,
    tracks_by_id: HashMap<String, usize>,
    tracks: Vec<WebDavTrack>,
    artists: Vec<Artist>,
    albums: Vec<Album>,
    album_tracks: HashMap<String, Vec<usize>>,
    artist_albums: HashMap<String, Vec<String>>,
    artist_tracks: HashMap<String, Vec<usize>>,
    genres: Vec<Genre>,
    covers: HashMap<String, String>,
}

impl WebDavLibraryIndex {
    pub(super) fn load(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        let Ok(persisted) = serde_json::from_str::<PersistedWebDavIndex>(&text) else {
            return Self::default();
        };
        if persisted.version != INDEX_SCHEMA_VERSION {
            return Self::default();
        }
        Self::from_tracks(persisted.tracks)
    }

    pub(super) fn save(&self, path: &Path) -> Result<()> {
        let persisted = PersistedWebDavIndex {
            version: INDEX_SCHEMA_VERSION,
            tracks: self.tracks.clone(),
        };
        crate::config::write_json_atomic(path, &persisted, "WebDAV library index")
    }

    pub(super) fn existing_track(&self, url: &str) -> Option<&WebDavTrack> {
        self.tracks_by_url.get(url)
    }

    pub(super) fn track_count(&self) -> usize {
        self.tracks.len()
    }

    pub(super) fn file_url(&self, song_id: &str) -> Option<&str> {
        let &idx = self.tracks_by_id.get(song_id)?;
        Some(self.tracks[idx].file_url.as_str())
    }

    pub(super) fn lrc_url(&self, song_id: &str) -> Option<&str> {
        let &idx = self.tracks_by_id.get(song_id)?;
        self.tracks[idx].lrc_url.as_deref()
    }

    pub(super) fn cover_url(&self, cover_id: &str) -> Option<&str> {
        self.covers.get(cover_id).map(String::as_str)
    }

    pub(super) fn from_tracks(mut tracks: Vec<WebDavTrack>) -> Self {
        tracks.sort_by(|a, b| a.file_url.cmp(&b.file_url));

        let mut tracks_by_url = HashMap::with_capacity(tracks.len());
        let mut tracks_by_id = HashMap::with_capacity(tracks.len());
        let mut covers: HashMap<String, String> = HashMap::new();

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
            created: Option<String>,
        }

        let mut artist_map: HashMap<String, ArtistAgg> = HashMap::new();
        let mut album_map: HashMap<String, AlbumAgg> = HashMap::new();
        let mut genre_songs: BTreeMap<String, (i64, HashSet<String>)> = BTreeMap::new();

        for (idx, track) in tracks.iter().enumerate() {
            tracks_by_url.insert(track.file_url.clone(), track.clone());
            tracks_by_id.insert(track.song.id.clone(), idx);
            if let (Some(cover_id), Some(url)) = (&track.song.cover_art, &track.cover_url) {
                covers.entry(cover_id.clone()).or_insert_with(|| url.clone());
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
                .unwrap_or_else(|| stable_id("wdartist", &artist_name.to_lowercase()));
            let album_name = if track.song.album.trim().is_empty() {
                "Unknown Album"
            } else {
                track.song.album.trim()
            };
            let album_id = track
                .song
                .album_id
                .clone()
                .unwrap_or_else(|| stable_id("wdalbum", &format!("{artist_id}\0{}", album_name.to_lowercase())));

            let artist_entry = artist_map.entry(artist_id.clone()).or_insert_with(|| ArtistAgg {
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

            let album_entry = album_map.entry(album_id.clone()).or_insert_with(|| AlbumAgg {
                id: album_id.clone(),
                name: album_name.to_string(),
                artist: artist_name.to_string(),
                artist_id: artist_id.clone(),
                cover_art: None,
                track_indices: Vec::new(),
                duration: 0,
                year: None,
                genre: None,
                created: track.last_modified.clone(),
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

            if let Some(genre) = track
                .song
                .genre
                .as_deref()
                .map(str::trim)
                .filter(|g| !g.is_empty())
            {
                let entry = genre_songs
                    .entry(genre.to_string())
                    .or_insert_with(|| (0, HashSet::new()));
                entry.0 += 1;
                entry.1.insert(album_id);
            }
        }

        let mut artists: Vec<Artist> = Vec::with_capacity(artist_map.len());
        let mut artist_albums_map = HashMap::new();
        let mut artist_tracks_map = HashMap::new();
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
        let mut album_tracks_map = HashMap::with_capacity(album_map.len());
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
                created: agg.created,
            });
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
            tracks_by_url,
            tracks_by_id,
            tracks,
            artists,
            albums,
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
                    b.created
                        .cmp(&a.created)
                        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });
            }
            AlbumSort::Name => albums.sort_by_key(|a| a.name.to_lowercase()),
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

    pub(super) fn resolve_favorites(&self, store: &WebDavFavoritesStore) -> Favorites {
        Favorites {
            artists: self
                .artists
                .iter()
                .filter(|artist| store.artists.contains(&artist.id))
                .cloned()
                .collect(),
            albums: self
                .albums
                .iter()
                .filter(|album| store.albums.contains(&album.id))
                .cloned()
                .collect(),
            songs: self
                .tracks
                .iter()
                .filter(|track| store.songs.contains(&track.song.id))
                .map(|track| track.song.clone())
                .collect(),
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
        SearchResults {
            artists: self
                .artists
                .iter()
                .filter(|artist| artist.name.to_lowercase().contains(&needle))
                .take(artist_limit)
                .cloned()
                .collect(),
            albums: self
                .albums
                .iter()
                .filter(|album| {
                    album.name.to_lowercase().contains(&needle)
                        || album.artist.to_lowercase().contains(&needle)
                })
                .take(album_limit)
                .cloned()
                .collect(),
            songs: self
                .tracks
                .iter()
                .filter(|track| {
                    track.song.title.to_lowercase().contains(&needle)
                        || track.song.artist.to_lowercase().contains(&needle)
                        || track.song.album.to_lowercase().contains(&needle)
                })
                .take(song_limit)
                .map(|track| track.song.clone())
                .collect(),
        }
    }

    pub(super) fn folder_listing(&self, root_path: &str, folder_id: &str) -> FolderListing {
        let prefix = folder_id.trim().trim_matches('/');
        let root_trim = root_path.trim_end_matches('/');
        let mut child_dirs: BTreeMap<String, (String, usize, Option<String>)> = BTreeMap::new();
        let mut direct_songs: Vec<Song> = Vec::new();

        for track in &self.tracks {
            let full = track.song.path.as_deref().unwrap_or_default();
            let rel = full
                .strip_prefix(root_trim)
                .unwrap_or(full)
                .trim_matches('/');
            let under_prefix = if prefix.is_empty() {
                rel
            } else {
                let Some(rest) = rel
                    .strip_prefix(prefix)
                    .and_then(|r| r.strip_prefix('/'))
                else {
                    continue;
                };
                rest
            };
            if under_prefix.is_empty() {
                continue;
            }
            if let Some((dir_name, _)) = under_prefix.split_once('/') {
                let child_id = if prefix.is_empty() {
                    dir_name.to_string()
                } else {
                    format!("{prefix}/{dir_name}")
                };
                let entry = child_dirs
                    .entry(child_id)
                    .or_insert_with(|| (dir_name.to_string(), 0, None));
                entry.1 += 1;
                if entry.2.is_none() {
                    entry.2.clone_from(&track.song.cover_art);
                }
            } else {
                direct_songs.push(track.song.clone());
            }
        }

        let mut folders: Vec<FolderItem> = child_dirs
            .into_iter()
            .map(|(id, (name, count, cover_art))| FolderItem {
                id,
                name,
                cover_art,
                song_count: Some(count),
            })
            .collect();
        folders.sort_by_key(|f| f.name.to_lowercase());
        direct_songs.sort_by(|a, b| {
            (
                a.disc.unwrap_or(1),
                a.track.unwrap_or(i32::MAX),
                a.title.to_lowercase(),
            )
                .cmp(&(
                    b.disc.unwrap_or(1),
                    b.track.unwrap_or(i32::MAX),
                    b.title.to_lowercase(),
                ))
        });

        let name = prefix
            .rsplit('/')
            .next()
            .unwrap_or_default()
            .to_string();

        FolderListing {
            id: prefix.to_string(),
            name,
            folders,
            songs: direct_songs,
        }
    }
}

/// Infer track/album/artist metadata from WebDAV directory path and filename,
/// overridden by any audio tags extracted via partial HTTP `Range` peek.
pub(super) fn infer_metadata_from_path(
    source_id: &SourceId,
    entry: &DavEntry,
    root_path: &str,
    cover_url: Option<String>,
    lrc_url: Option<String>,
    peeked: Option<PeekedAudioTags>,
) -> WebDavTrack {
    let suffix = entry
        .file_name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase());
    let stem = entry
        .file_name
        .rsplit_once('.')
        .map(|(s, _)| s)
        .unwrap_or(&entry.file_name)
        .trim();

    let (parsed_track_no, stem_without_num) = split_leading_track_number(stem);
    let (inferred_artist_from_file, inferred_title) = split_artist_title(stem_without_num);

    // Strip configured root_path prefix to extract relative path segments.
    let relative = entry
        .decoded_path
        .strip_prefix(root_path.trim_end_matches('/'))
        .unwrap_or(&entry.decoded_path)
        .trim_matches('/');
    let segments: Vec<&str> = relative.split('/').filter(|s| !s.is_empty()).collect();
    let dir_segments = if segments.len() > 1 {
        &segments[..segments.len() - 1]
    } else {
        &[]
    };

    let inferred_album = dir_segments
        .last()
        .copied()
        .unwrap_or("Unknown Album")
        .to_string();
    let inferred_artist = if let Some(artist) = inferred_artist_from_file {
        artist.to_string()
    } else if dir_segments.len() >= 2 {
        dir_segments[dir_segments.len() - 2].to_string()
    } else {
        "Unknown Artist".to_string()
    };

    let tags = peeked.unwrap_or_default();
    let title = tags.title.unwrap_or_else(|| inferred_title.to_string());
    let artist = tags.artist.unwrap_or(inferred_artist);
    let album_artist = tags.album_artist.unwrap_or_else(|| artist.clone());
    let album = tags.album.unwrap_or(inferred_album);
    let track = tags.track.or(parsed_track_no);

    let file_url = entry.url.to_string();
    let song_id = stable_id("wdtrack", &file_url);
    let artist_id = stable_id("wdartist", &album_artist.to_lowercase());
    let album_id = stable_id("wdalbum", &format!("{artist_id}\0{}", album.to_lowercase()));
    let cover_art = cover_url.as_deref().map(|u| stable_id("wdcover", u));
    let content_type = suffix
        .as_deref()
        .and_then(|ext| {
            crate::source::local::mime_for_path(Path::new(&format!("track.{ext}")))
        })
        .map(str::to_string);

    let song = Song {
        source: Some(source_id.clone()),
        id: song_id,
        title,
        bilibili_part: None,
        album,
        album_id: Some(album_id),
        artist,
        artist_id: Some(artist_id),
        genre: tags.genre,
        track,
        disc: tags.disc,
        duration: tags.duration,
        cover_art,
        suffix,
        content_type,
        bit_rate: tags.bit_rate,
        size: entry.content_length.and_then(|n| i64::try_from(n).ok()),
        path: Some(entry.decoded_path.clone()),
        replay_gain_track_gain: None,
        replay_gain_track_peak: None,
        replay_gain_album_gain: None,
        replay_gain_album_peak: None,
    };

    WebDavTrack {
        file_url,
        etag: entry.etag.clone(),
        content_length: entry.content_length,
        last_modified: entry.last_modified.clone(),
        year: tags.year,
        album_artist,
        cover_url,
        lrc_url,
        song,
    }
}

fn split_leading_track_number(stem: &str) -> (Option<i32>, &str) {
    let trimmed = stem.trim();
    let digit_end = trimmed
        .bytes()
        .take_while(|b| b.is_ascii_digit())
        .count();
    if digit_end == 0 || digit_end > 3 {
        return (None, trimmed);
    }
    let rest = &trimmed[digit_end..];
    let stripped = rest
        .strip_prefix(" - ")
        .or_else(|| rest.strip_prefix(". "))
        .or_else(|| rest.strip_prefix('.'))
        .or_else(|| rest.strip_prefix('-'))
        .or_else(|| rest.strip_prefix(' '));
    let Some(tail) = stripped.map(str::trim).filter(|t| !t.is_empty()) else {
        return (None, trimmed);
    };
    let num = trimmed[..digit_end].parse::<i32>().ok().filter(|n| *n > 0);
    (num, tail)
}

fn split_artist_title(input: &str) -> (Option<&str>, &str) {
    if let Some((left, right)) = input.split_once(" - ") {
        let left = left.trim();
        let right = right.trim();
        if !left.is_empty() && !right.is_empty() {
            return (Some(left), right);
        }
    }
    (None, input.trim())
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct WebDavFavoritesStore {
    pub(super) songs: BTreeSet<String>,
    pub(super) albums: BTreeSet<String>,
    pub(super) artists: BTreeSet<String>,
}

impl WebDavFavoritesStore {
    pub(super) fn load(path: &Path) -> Self {
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    pub(super) fn save(&self, path: &Path) -> Result<()> {
        crate::config::write_json_atomic(path, self, "WebDAV favorites")
    }

    pub(super) fn set(&mut self, key: &FavoriteKey, starred: bool) {
        let target = match key.kind {
            FavoriteKind::Song => &mut self.songs,
            FavoriteKind::Album => &mut self.albums,
            FavoriteKind::Artist => &mut self.artists,
        };
        if starred {
            target.insert(key.id.clone());
        } else {
            target.remove(&key.id);
        }
    }
}

pub(super) fn stable_id(prefix: &str, key: &str) -> String {
    let mut hash = Md5::new();
    hash.update(prefix.as_bytes());
    hash.update([0]);
    hash.update(key.as_bytes());
    format!(
        "{prefix}-{:016x}",
        u64::from_be_bytes(hash.finalize()[..8].try_into().unwrap())
    )
}
