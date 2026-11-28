//! Manifest store, offline library queries, and protected file lifecycle.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::models::{
    Album, AlbumSort, Artist, Genre, Playlist, SearchResults, Song, TranscodingQuality,
};
use crate::source::{MediaKey, SourceId};

const MANIFEST_VERSION: u32 = 1;
const MANIFEST_FILE: &str = "offline-manifest.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OfflineTrackStatus {
    NotPinned,
    Downloading,
    Ready,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct OfflineEntry {
    pub(crate) media: MediaKey,
    pub(crate) song: Song,
    pub(crate) file_name: String,
    pub(crate) quality: TranscodingQuality,
    pub(crate) bytes: u64,
    pub(crate) pinned_at: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct OfflinePlaylistSnapshot {
    pub(crate) source: SourceId,
    pub(crate) playlist: Playlist,
    pub(crate) song_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct OfflineStats {
    pub(crate) track_count: usize,
    pub(crate) total_bytes: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct PersistedManifest {
    version: u32,
    entries: Vec<OfflineEntry>,
    playlists: Vec<OfflinePlaylistSnapshot>,
}

#[derive(Clone, Debug)]
pub(crate) struct OfflineStore {
    dir: PathBuf,
    entries: HashMap<MediaKey, OfflineEntry>,
    playlists: HashMap<(SourceId, String), OfflinePlaylistSnapshot>,
    downloading: HashSet<MediaKey>,
}

impl OfflineStore {
    pub(crate) fn open(dir: PathBuf) -> Self {
        let mut store = Self {
            dir,
            entries: HashMap::new(),
            playlists: HashMap::new(),
            downloading: HashSet::new(),
        };
        store.reload();
        store
    }

    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    pub(crate) fn set_dir(&mut self, dir: PathBuf) {
        if self.dir == dir {
            return;
        }
        self.dir = dir;
        self.entries.clear();
        self.playlists.clear();
        self.downloading.clear();
        self.reload();
    }

    pub(crate) fn reload(&mut self) {
        let manifest_path = self.dir.join(MANIFEST_FILE);
        let Ok(text) = fs::read_to_string(&manifest_path) else {
            return;
        };
        let Ok(persisted) = serde_json::from_str::<PersistedManifest>(&text) else {
            log::warn!(
                "ignoring malformed offline manifest at {}",
                manifest_path.display()
            );
            return;
        };
        if persisted.version != MANIFEST_VERSION {
            return;
        }
        self.entries.clear();
        for mut entry in persisted.entries {
            let file_path = self.dir.join(&entry.file_name);
            if file_path.is_file() {
                entry.song.source = Some(entry.media.source.clone());
                self.entries.insert(entry.media.clone(), entry);
            }
        }
        self.playlists.clear();
        for snap in persisted.playlists {
            self.playlists
                .insert((snap.source.clone(), snap.playlist.id.clone()), snap);
        }
    }

    pub(crate) fn save(&self) -> Result<()> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("failed to create offline dir {}", self.dir.display()))?;
        let mut entries: Vec<OfflineEntry> = self.entries.values().cloned().collect();
        entries.sort_by(|a, b| a.pinned_at.cmp(&b.pinned_at).then(a.file_name.cmp(&b.file_name)));
        let mut playlists: Vec<OfflinePlaylistSnapshot> = self.playlists.values().cloned().collect();
        playlists.sort_by(|a, b| a.playlist.id.cmp(&b.playlist.id));
        let persisted = PersistedManifest {
            version: MANIFEST_VERSION,
            entries,
            playlists,
        };
        crate::config::write_json_atomic(
            &self.dir.join(MANIFEST_FILE),
            &persisted,
            "offline manifest",
        )
    }

    pub(crate) fn status(&self, media: &MediaKey) -> OfflineTrackStatus {
        if self.entries.contains_key(media) {
            OfflineTrackStatus::Ready
        } else if self.downloading.contains(media) {
            OfflineTrackStatus::Downloading
        } else {
            OfflineTrackStatus::NotPinned
        }
    }

    pub(crate) fn audio_path(&self, media: &MediaKey) -> Option<PathBuf> {
        let entry = self.entries.get(media)?;
        let path = self.dir.join(&entry.file_name);
        path.is_file().then_some(path)
    }

    pub(crate) fn mark_downloading(&mut self, media: MediaKey) -> bool {
        if self.entries.contains_key(&media) {
            return false;
        }
        self.downloading.insert(media)
    }

    pub(crate) fn finish_download(&mut self, media: &MediaKey, entry: Option<OfflineEntry>) -> Result<()> {
        self.downloading.remove(media);
        if let Some(mut entry) = entry {
            entry.song.source = Some(entry.media.source.clone());
            self.entries.insert(entry.media.clone(), entry);
            self.save()?;
        }
        Ok(())
    }

    pub(crate) fn record_playlist_snapshot(
        &mut self,
        source: SourceId,
        playlist: Playlist,
        songs: &[Song],
    ) -> Result<()> {
        let snap = OfflinePlaylistSnapshot {
            source: source.clone(),
            playlist: playlist.clone(),
            song_ids: songs.iter().map(|s| s.id.clone()).collect(),
        };
        self.playlists.insert((source, playlist.id), snap);
        self.save()
    }

    pub(crate) fn remove_track(&mut self, media: &MediaKey) -> Result<bool> {
        self.downloading.remove(media);
        let Some(removed) = self.entries.remove(media) else {
            return Ok(false);
        };
        let path = self.dir.join(&removed.file_name);
        if path.is_file() {
            let _ = fs::remove_file(&path);
        }
        self.prune_empty_playlists();
        self.save()?;
        Ok(true)
    }

    pub(crate) fn remove_tracks(&mut self, keys: &[MediaKey]) -> Result<usize> {
        let mut count = 0;
        for key in keys {
            self.downloading.remove(key);
            if let Some(removed) = self.entries.remove(key) {
                let path = self.dir.join(&removed.file_name);
                if path.is_file() {
                    let _ = fs::remove_file(&path);
                }
                count += 1;
            }
        }
        if count > 0 {
            self.prune_empty_playlists();
            self.save()?;
        }
        Ok(count)
    }

    pub(crate) fn clear_all(&mut self) -> Result<(usize, u64)> {
        let stats = self.stats(None);
        for entry in self.entries.values() {
            let path = self.dir.join(&entry.file_name);
            let _ = fs::remove_file(&path);
        }
        self.entries.clear();
        self.playlists.clear();
        self.downloading.clear();
        self.save()?;
        Ok((stats.track_count, stats.total_bytes))
    }

    pub(crate) fn downloading_count(&self) -> usize {
        self.downloading.len()
    }

    pub(crate) fn stats(&self, source: Option<&SourceId>) -> OfflineStats {
        let mut stats = OfflineStats::default();
        for entry in self.entries.values() {
            if source.is_some_and(|s| &entry.media.source != s) {
                continue;
            }
            stats.track_count += 1;
            stats.total_bytes = stats.total_bytes.saturating_add(entry.bytes);
        }
        stats
    }

    pub(crate) fn songs_for_source(&self, source: &SourceId) -> Vec<Song> {
        let mut entries: Vec<&OfflineEntry> = self
            .entries
            .values()
            .filter(|entry| &entry.media.source == source)
            .collect();
        entries.sort_by(|a, b| {
            a.song
                .album
                .to_lowercase()
                .cmp(&b.song.album.to_lowercase())
                .then_with(|| a.song.disc.unwrap_or(1).cmp(&b.song.disc.unwrap_or(1)))
                .then_with(|| {
                    a.song
                        .track
                        .unwrap_or(i32::MAX)
                        .cmp(&b.song.track.unwrap_or(i32::MAX))
                })
                .then_with(|| a.song.title.to_lowercase().cmp(&b.song.title.to_lowercase()))
        });
        entries.into_iter().map(|e| e.song.clone()).collect()
    }

    pub(crate) fn artists(&self, source: &SourceId) -> Vec<Artist> {
        let mut by_artist: HashMap<String, (Artist, HashSet<String>)> = HashMap::new();
        for entry in self
            .entries
            .values()
            .filter(|e| &e.media.source == source)
        {
            let name = if entry.song.artist.trim().is_empty() {
                "Unknown Artist".to_string()
            } else {
                entry.song.artist.trim().to_string()
            };
            let id = entry
                .song
                .artist_id
                .clone()
                .unwrap_or_else(|| format!("offline-artist:{}", name.to_lowercase()));
            let album_key = entry
                .song
                .album_id
                .clone()
                .unwrap_or_else(|| format!("offline-album:{}", entry.song.album.to_lowercase()));
            let slot = by_artist.entry(id.clone()).or_insert_with(|| {
                (
                    Artist {
                        id,
                        name,
                        album_count: None,
                        cover_art: entry.song.cover_art.clone(),
                    },
                    HashSet::new(),
                )
            });
            if slot.0.cover_art.is_none() {
                slot.0.cover_art.clone_from(&entry.song.cover_art);
            }
            slot.1.insert(album_key);
        }
        let mut artists: Vec<Artist> = by_artist
            .into_values()
            .map(|(mut artist, albums)| {
                artist.album_count = i32::try_from(albums.len()).ok();
                artist
            })
            .collect();
        artists.sort_by_key(|a| a.name.to_lowercase());
        artists
    }

    pub(crate) fn albums(&self, source: &SourceId, sort: AlbumSort) -> Vec<Album> {
        let mut by_album: HashMap<String, (Album, u64)> = HashMap::new();
        for entry in self
            .entries
            .values()
            .filter(|e| &e.media.source == source)
        {
            let name = if entry.song.album.trim().is_empty() {
                "Unknown Album".to_string()
            } else {
                entry.song.album.trim().to_string()
            };
            let id = entry
                .song
                .album_id
                .clone()
                .unwrap_or_else(|| format!("offline-album:{}", name.to_lowercase()));
            let slot = by_album.entry(id.clone()).or_insert_with(|| {
                (
                    Album {
                        id,
                        name,
                        artist: entry.song.artist.clone(),
                        artist_id: entry.song.artist_id.clone(),
                        cover_art: entry.song.cover_art.clone(),
                        song_count: Some(0),
                        duration: Some(0),
                        year: None,
                        genre: entry.song.genre.clone(),
                        created: chrono::DateTime::from_timestamp(entry.pinned_at as i64, 0)
                            .map(|dt| dt.to_rfc3339()),
                    },
                    entry.pinned_at,
                )
            });
            slot.0.song_count = Some(slot.0.song_count.unwrap_or(0) + 1);
            slot.0.duration = Some(
                slot.0
                    .duration
                    .unwrap_or(0)
                    .saturating_add(entry.song.duration.unwrap_or(0).max(0)),
            );
            if slot.0.cover_art.is_none() {
                slot.0.cover_art.clone_from(&entry.song.cover_art);
            }
            slot.1 = slot.1.max(entry.pinned_at);
        }
        let mut albums: Vec<(Album, u64)> = by_album.into_values().collect();
        match sort {
            AlbumSort::NewestAdded => albums.sort_by(|a, b| {
                b.1.cmp(&a.1)
                    .then_with(|| a.0.name.to_lowercase().cmp(&b.0.name.to_lowercase()))
            }),
            AlbumSort::Name => albums.sort_by_key(|a| a.0.name.to_lowercase()),
            AlbumSort::Artist => albums.sort_by(|a, b| {
                a.0.artist
                    .to_lowercase()
                    .cmp(&b.0.artist.to_lowercase())
                    .then_with(|| a.0.name.to_lowercase().cmp(&b.0.name.to_lowercase()))
            }),
            AlbumSort::Year => albums.sort_by(|a, b| {
                b.0.year
                    .unwrap_or(0)
                    .cmp(&a.0.year.unwrap_or(0))
                    .then_with(|| a.0.name.to_lowercase().cmp(&b.0.name.to_lowercase()))
            }),
        }
        albums.into_iter().map(|(album, _)| album).collect()
    }

    pub(crate) fn artist_albums(&self, source: &SourceId, artist_id: &str) -> Vec<Album> {
        self.albums(source, AlbumSort::Name)
            .into_iter()
            .filter(|album| {
                album.artist_id.as_deref() == Some(artist_id)
                    || format!("offline-artist:{}", album.artist.to_lowercase()) == artist_id
            })
            .collect()
    }

    pub(crate) fn artist_songs(&self, source: &SourceId, artist: &Artist) -> Vec<Song> {
        self.songs_for_source(source)
            .into_iter()
            .filter(|song| {
                song.artist_id.as_deref() == Some(artist.id.as_str())
                    || song.artist.eq_ignore_ascii_case(&artist.name)
            })
            .collect()
    }

    pub(crate) fn album_songs(&self, source: &SourceId, album_id: &str) -> Vec<Song> {
        let mut songs: Vec<Song> = self
            .entries
            .values()
            .filter(|entry| {
                &entry.media.source == source
                    && (entry.song.album_id.as_deref() == Some(album_id)
                        || format!("offline-album:{}", entry.song.album.to_lowercase()) == album_id)
            })
            .map(|entry| entry.song.clone())
            .collect();
        songs.sort_by(|a, b| {
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
        songs
    }

    pub(crate) fn genres(&self, source: &SourceId) -> Vec<Genre> {
        let mut counts: BTreeMap<String, (i64, HashSet<String>)> = BTreeMap::new();
        for entry in self
            .entries
            .values()
            .filter(|e| &e.media.source == source)
        {
            if let Some(genre) = entry
                .song
                .genre
                .as_deref()
                .map(str::trim)
                .filter(|g| !g.is_empty())
            {
                let item = counts
                    .entry(genre.to_string())
                    .or_insert_with(|| (0, HashSet::new()));
                item.0 += 1;
                if let Some(album_id) = &entry.song.album_id {
                    item.1.insert(album_id.clone());
                }
            }
        }
        counts
            .into_iter()
            .map(|(value, (songs, albums))| Genre {
                value,
                song_count: Some(songs),
                album_count: Some(albums.len() as i64),
            })
            .collect()
    }

    pub(crate) fn albums_by_genre(&self, source: &SourceId, genre: &str) -> Vec<Album> {
        let needle = genre.trim().to_lowercase();
        self.albums(source, AlbumSort::Name)
            .into_iter()
            .filter(|album| {
                album
                    .genre
                    .as_deref()
                    .is_some_and(|g| g.trim().to_lowercase() == needle)
            })
            .collect()
    }

    pub(crate) fn playlists(&self, source: &SourceId) -> Vec<Playlist> {
        let mut result: Vec<Playlist> = self
            .playlists
            .values()
            .filter(|snap| &snap.source == source)
            .filter_map(|snap| {
                let available = snap
                    .song_ids
                    .iter()
                    .filter(|id| {
                        self.entries
                            .contains_key(&MediaKey::new(source.clone(), id.as_str()))
                    })
                    .count();
                if available == 0 {
                    return None;
                }
                let mut pl = snap.playlist.clone();
                pl.song_count = i32::try_from(available).ok();
                Some(pl)
            })
            .collect();
        result.sort_by_key(|p| p.name.to_lowercase());
        result
    }

    pub(crate) fn playlist_songs(&self, source: &SourceId, playlist_id: &str) -> Vec<Song> {
        let Some(snap) = self.playlists.get(&(source.clone(), playlist_id.to_string())) else {
            return Vec::new();
        };
        snap.song_ids
            .iter()
            .filter_map(|id| {
                self.entries
                    .get(&MediaKey::new(source.clone(), id.as_str()))
                    .map(|entry| entry.song.clone())
            })
            .collect()
    }

    pub(crate) fn search(&self, source: &SourceId, query: &str) -> SearchResults {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return SearchResults::default();
        }
        let artists = self
            .artists(source)
            .into_iter()
            .filter(|a| a.name.to_lowercase().contains(&needle))
            .collect();
        let albums = self
            .albums(source, AlbumSort::Name)
            .into_iter()
            .filter(|a| {
                a.name.to_lowercase().contains(&needle) || a.artist.to_lowercase().contains(&needle)
            })
            .collect();
        let songs = self
            .songs_for_source(source)
            .into_iter()
            .filter(|s| {
                s.title.to_lowercase().contains(&needle)
                    || s.artist.to_lowercase().contains(&needle)
                    || s.album.to_lowercase().contains(&needle)
            })
            .collect();
        SearchResults {
            artists,
            albums,
            songs,
        }
    }

    fn prune_empty_playlists(&mut self) {
        self.playlists.retain(|_, snap| {
            snap.song_ids.iter().any(|id| {
                self.entries
                    .contains_key(&MediaKey::new(snap.source.clone(), id.as_str()))
            })
        });
    }
}

pub(crate) fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(crate) fn offline_file_name(media: &MediaKey, suffix: Option<&str>) -> String {
    let ext = suffix
        .map(str::trim)
        .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric()))
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "audio".to_string());
    format!("{}.{ext}", media.cache_key())
}
