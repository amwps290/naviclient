//! Local filesystem music folder source (`ProviderKind::LocalFolder`).
//!
//! Scans configured local directories recursively with incremental `(mtime, size)`
//! caching, parses audio tags and embedded artwork via `lofty`, reads local
//! `.lrc`/embedded lyrics, persists local favorites, and resolves playback directly
//! to local files without network I/O.

mod favorites;
mod index;
mod lyrics;
mod scanner;

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{anyhow, Result};
use rand::seq::SliceRandom;

use crate::models::{
    Album, AlbumSort, Artist, FavoriteKey, Favorites, FolderListing, Genre, Lyrics, SearchResults,
    ServerInfo, Song,
};
use crate::source::{
    AudioQuality, HttpResource, MediaKey, PlaybackResource, SourceId,
};

use favorites::LocalFavoritesStore;
use index::{CoverOrigin, LocalLibraryIndex};
use scanner::scan_roots;

#[derive(Clone)]
pub struct LocalFolderProvider {
    id: SourceId,
    name: String,
    roots: Vec<PathBuf>,
    data_dir: PathBuf,
    index: Arc<RwLock<LocalLibraryIndex>>,
    favorites: Arc<RwLock<LocalFavoritesStore>>,
}

impl LocalFolderProvider {
    pub fn new(id: SourceId, name: String, roots: Vec<PathBuf>) -> Self {
        let data_dir = crate::config::source_data_dir(&id);
        Self::with_data_dir(id, name, roots, data_dir)
    }

    pub fn with_data_dir(
        id: SourceId,
        name: String,
        roots: Vec<PathBuf>,
        data_dir: PathBuf,
    ) -> Self {
        let index = LocalLibraryIndex::load(&data_dir.join("library-index.json"));
        let favorites = LocalFavoritesStore::load(&data_dir.join("favorites.json"));
        Self {
            id,
            name,
            roots,
            data_dir,
            index: Arc::new(RwLock::new(index)),
            favorites: Arc::new(RwLock::new(favorites)),
        }
    }

    pub fn id(&self) -> &SourceId {
        &self.id
    }

    pub fn name(&self) -> &str {
        if self.name.trim().is_empty() {
            "Local Music"
        } else {
            self.name.trim()
        }
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    pub fn base_label(&self) -> String {
        match self.roots.as_slice() {
            [] => String::new(),
            [single] => single.display().to_string(),
            [first, ..] => format!("{} (+{})", first.display(), self.roots.len() - 1),
        }
    }

    pub fn track_count(&self) -> usize {
        self.index
            .read()
            .map(|guard| guard.track_count())
            .unwrap_or(0)
    }

    /// Perform an incremental scan of all configured roots and persist the index.
    pub fn rescan(&self) -> Result<ScanSummary> {
        let existing = self
            .index
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        let (updated, summary) = scan_roots(&self.id, &self.roots, &existing);
        if let Err(error) = updated.save(&self.data_dir.join("library-index.json")) {
            log::warn!(
                "failed to save local library index for {}: {error:#}",
                self.id.profile_id
            );
        }
        if let Ok(mut guard) = self.index.write() {
            *guard = updated;
        }
        Ok(summary)
    }

    pub async fn ping(&self) -> Result<ServerInfo> {
        if self.roots.is_empty() {
            anyhow::bail!("No folders configured for this local music source");
        }
        let existing_roots = self.roots.iter().filter(|root| root.is_dir()).count();
        if existing_roots == 0 {
            anyhow::bail!("None of the configured music folders exist on disk");
        }
        let this = self.clone();
        let summary = tokio::task::spawn_blocking(move || this.rescan())
            .await
            .map_err(|error| anyhow!("Local library scan interrupted: {error}"))??;
        let mut info = ServerInfo {
            app_name: Some(self.name().to_string()),
            server_version: Some(format!("{} tracks", summary.total_tracks)),
            protocol_version: "local".into(),
            extensions_discovered: true,
            ..Default::default()
        };
        info.refresh_capabilities();
        Ok(info)
    }

    pub fn artists(&self) -> Vec<Artist> {
        self.index
            .read()
            .map(|guard| guard.artists())
            .unwrap_or_default()
    }

    pub fn artist_albums(&self, artist_id: &str) -> Vec<Album> {
        self.index
            .read()
            .map(|guard| guard.artist_albums(artist_id))
            .unwrap_or_default()
    }

    pub fn artist_top_songs(&self, artist: &Artist, count: u32) -> Vec<Song> {
        self.index
            .read()
            .map(|guard| guard.artist_songs(&artist.id, &artist.name, count as usize))
            .unwrap_or_default()
    }

    pub fn albums(&self, size: u32, offset: u32, sort: AlbumSort) -> Vec<Album> {
        self.index
            .read()
            .map(|guard| guard.albums(size as usize, offset as usize, sort))
            .unwrap_or_default()
    }

    pub fn albums_newest(&self, size: u32) -> Vec<Album> {
        self.albums(size, 0, AlbumSort::NewestAdded)
    }

    pub fn albums_recent(&self, size: u32) -> Vec<Album> {
        // Local folders do not track remote play timestamps; show recently added albums.
        self.albums(size, 0, AlbumSort::NewestAdded)
    }

    pub fn albums_frequent(&self, size: u32) -> Vec<Album> {
        self.albums(size, 0, AlbumSort::Name)
    }

    pub fn albums_random(&self, size: u32) -> Vec<Album> {
        let mut albums = self
            .index
            .read()
            .map(|guard| guard.all_albums())
            .unwrap_or_default();
        let mut rng = rand::thread_rng();
        albums.shuffle(&mut rng);
        albums.truncate(size as usize);
        albums
    }

    pub fn genres(&self) -> Vec<Genre> {
        self.index
            .read()
            .map(|guard| guard.genres())
            .unwrap_or_default()
    }

    pub fn albums_by_genre(&self, genre: &str, size: u32, offset: u32) -> Vec<Album> {
        self.index
            .read()
            .map(|guard| guard.albums_by_genre(genre, size as usize, offset as usize))
            .unwrap_or_default()
    }

    pub fn album_songs(&self, album_id: &str) -> Vec<Song> {
        self.index
            .read()
            .map(|guard| guard.album_songs(album_id))
            .unwrap_or_default()
    }

    pub fn songs_by_genre(&self, genre: &str, count: u32, offset: u32) -> Vec<Song> {
        self.index
            .read()
            .map(|guard| guard.songs_by_genre(genre, count as usize, offset as usize))
            .unwrap_or_default()
    }

    pub fn favorites(&self) -> Favorites {
        let favs = self
            .favorites
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        let guard = match self.index.read() {
            Ok(guard) => guard,
            Err(_) => return Favorites::default(),
        };
        guard.resolve_favorites(&favs)
    }

    pub fn set_favorite(&self, key: &FavoriteKey, starred: bool) -> Result<()> {
        let path = self.data_dir.join("favorites.json");
        let mut guard = self
            .favorites
            .write()
            .map_err(|_| anyhow!("local favorites lock poisoned"))?;
        guard.set(key, starred);
        guard.save(&path)
    }

    pub fn search(
        &self,
        query: &str,
        songs: u32,
        albums: u32,
        artists: u32,
    ) -> SearchResults {
        self.index
            .read()
            .map(|guard| guard.search(query, songs as usize, albums as usize, artists as usize))
            .unwrap_or_default()
    }

    pub fn folder_listing(&self, folder_id: &str) -> FolderListing {
        self.index
            .read()
            .map(|guard| guard.folder_listing(&self.roots, folder_id))
            .unwrap_or_default()
    }

    pub fn lyrics(&self, song: &Song) -> Result<Lyrics> {
        let path = self
            .song_path(song)
            .ok_or_else(|| anyhow!("Local audio file not found for track {}", song.id))?;
        lyrics::load_local_lyrics(&path)
    }

    pub fn resolve_playback(&self, song: &Song) -> Result<PlaybackResource> {
        let path = self
            .song_path(song)
            .ok_or_else(|| anyhow!("Local audio file not found for track {}", song.id))?;
        if !path.is_file() {
            anyhow::bail!("Local audio file no longer exists: {}", path.display());
        }
        let mime_hint = mime_for_path(&path).map(str::to_string);
        let http = HttpResource::local_file(path).with_mime_hint(mime_hint);
        let codec = song
            .suffix
            .as_deref()
            .map(|suffix| suffix.to_ascii_uppercase());
        let bandwidth = song
            .bit_rate
            .and_then(|kbps| u64::try_from(kbps).ok())
            .map(|kbps| kbps.saturating_mul(1_000));
        let duration = song
            .duration
            .and_then(|secs| u64::try_from(secs).ok())
            .filter(|secs| *secs > 0)
            .map(Duration::from_secs);
        Ok(PlaybackResource {
            media: MediaKey::new(self.id.clone(), &song.id),
            representation: "local".to_string(),
            http,
            backups: Vec::new(),
            quality: AudioQuality {
                codec,
                bandwidth,
                sample_rate: None,
                channels: None,
            },
            duration,
        })
    }

    pub fn cover_url(&self, id: &str) -> Result<String> {
        if id.trim().is_empty() {
            anyhow::bail!("empty local cover id");
        }
        Ok(format!("local-cover://{id}"))
    }

    pub fn cover_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let cover_id = url.strip_prefix("local-cover://").unwrap_or(url);
        let origin = self
            .index
            .read()
            .ok()
            .and_then(|guard| guard.cover_origin(cover_id).cloned())
            .ok_or_else(|| anyhow!("Local cover art not found: {cover_id}"))?;
        match origin {
            CoverOrigin::ImageFile(path) => {
                fs::read(&path).map_err(|error| anyhow!("Failed to read cover {}: {error}", path.display()))
            }
            CoverOrigin::EmbeddedAudio(path) => scanner::extract_embedded_cover(&path)
                .ok_or_else(|| anyhow!("No embedded cover art in {}", path.display())),
        }
    }

    fn song_path(&self, song: &Song) -> Option<PathBuf> {
        if let Ok(guard) = self.index.read() {
            if let Some(path) = guard.song_path(&song.id) {
                return Some(path.to_path_buf());
            }
        }
        song.path
            .as_deref()
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScanSummary {
    pub total_tracks: usize,
    pub parsed_tracks: usize,
    pub reused_tracks: usize,
    pub skipped_files: usize,
}

pub(crate) fn mime_for_path(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "mp3" => Some("audio/mpeg"),
        "flac" => Some("audio/flac"),
        "ogg" | "oga" => Some("audio/ogg"),
        "opus" => Some("audio/opus"),
        "m4a" | "mp4" | "aac" | "alac" => Some("audio/mp4"),
        "wav" => Some("audio/wav"),
        "aif" | "aiff" => Some("audio/aiff"),
        "wma" => Some("audio/x-ms-wma"),
        _ => None,
    }
}

/// Normalize user-entered folder paths: trim whitespace, expand leading `~/`,
/// deduplicate while preserving order.
pub fn normalize_roots(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for root in roots {
        let raw = root.to_string_lossy();
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let expanded = expand_tilde(trimmed);
        if seen.insert(expanded.clone()) {
            normalized.push(expanded);
        }
    }
    normalized
}

fn expand_tilde(input: &str) -> PathBuf {
    if let Some(rest) = input.strip_prefix("~/") {
        if let Some(home) = directories::UserDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) {
            return home.join(rest);
        }
    } else if input == "~" {
        if let Some(home) = directories::UserDirs::new().map(|dirs| dirs.home_dir().to_path_buf()) {
            return home;
        }
    }
    PathBuf::from(input)
}
