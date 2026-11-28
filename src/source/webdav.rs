//! WebDAV / Alist / Nextcloud / NAS private storage music source (`ProviderKind::WebDav`).
//!
//! Discovers audio files recursively via WebDAV `PROPFIND` (`Depth: 1`, bounded concurrency),
//! persists an incremental index keyed by `(etag, content_length, last_modified)`, extracts
//! tags via partial HTTP `Range` peeks with smart directory/filename fallback, reads sibling
//! `.lrc` files and folder covers, and streams via authenticated `HttpResource` (supporting
//! gapless, seek, passive cache, and LP-02 offline pinning).

mod client;
mod index;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use anyhow::{anyhow, Result};
use rand::seq::SliceRandom;
use reqwest::header::HeaderMap;
use url::Url;

use crate::models::network::NetworkRoute;
use crate::models::{
    Album, AlbumSort, Artist, FavoriteKey, Favorites, Genre, Lyrics, SearchResults, ServerInfo,
    Song,
};
use crate::source::{AudioQuality, HttpResource, MediaKey, PlaybackResource, SourceId};

use client::{build_auth_headers, build_http_client, join_webdav_url, WebDavClient};
use index::{infer_metadata_from_path, WebDavFavoritesStore, WebDavLibraryIndex};

const MAX_SCAN_CONCURRENCY: usize = 4;
const MAX_SCAN_DIRECTORIES: usize = 2_000;
const MAX_TAG_PEEKS_PER_SCAN: usize = 250;

#[derive(Clone)]
pub struct WebDavProvider {
    id: SourceId,
    name: String,
    base_url: String,
    root_path: String,
    username: String,
    password: String,
    route: NetworkRoute,
    data_dir: PathBuf,
    client: WebDavClient,
    index: Arc<RwLock<WebDavLibraryIndex>>,
    favorites: Arc<RwLock<WebDavFavoritesStore>>,
}

impl WebDavProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: SourceId,
        name: String,
        base_url: String,
        root_path: String,
        username: String,
        password: String,
        route: NetworkRoute,
    ) -> Result<Self> {
        let data_dir = crate::config::source_data_dir(&id);
        Self::with_data_dir(
            id, name, base_url, root_path, username, password, route, data_dir,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_data_dir(
        id: SourceId,
        name: String,
        base_url: String,
        root_path: String,
        username: String,
        password: String,
        route: NetworkRoute,
        data_dir: PathBuf,
    ) -> Result<Self> {
        let normalized_base = normalize_webdav_url(&base_url)?;
        let normalized_root = normalize_root_path(&root_path);
        let http = build_http_client(&route)?;
        let client = WebDavClient::new(http, username.clone(), password.clone());
        let index = WebDavLibraryIndex::load(&data_dir.join("library-index.json"));
        let favorites = WebDavFavoritesStore::load(&data_dir.join("favorites.json"));
        Ok(Self {
            id,
            name,
            base_url: normalized_base,
            root_path: normalized_root,
            username,
            password,
            route,
            data_dir,
            client,
            index: Arc::new(RwLock::new(index)),
            favorites: Arc::new(RwLock::new(favorites)),
        })
    }

    pub fn id(&self) -> &SourceId {
        &self.id
    }

    pub fn name(&self) -> &str {
        if self.name.trim().is_empty() {
            "WebDAV"
        } else {
            self.name.trim()
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn track_count(&self) -> usize {
        self.index
            .read()
            .map(|guard| guard.track_count())
            .unwrap_or(0)
    }

    fn root_url(&self) -> Result<Url> {
        join_webdav_url(&self.base_url, &self.root_path)
    }

    pub fn auth_headers(&self) -> HeaderMap {
        build_auth_headers(&self.username, &self.password)
    }

    pub async fn ping(&self) -> Result<ServerInfo> {
        let summary = self.rescan().await?;
        let mut info = ServerInfo {
            app_name: Some(self.name().to_string()),
            server_version: Some(format!("{} tracks", summary.total_tracks)),
            protocol_version: "webdav".into(),
            extensions_discovered: true,
            ..Default::default()
        };
        info.refresh_capabilities();
        Ok(info)
    }

    /// Incrementally crawl the configured WebDAV directory via `PROPFIND Depth: 1`.
    pub async fn rescan(&self) -> Result<WebDavScanSummary> {
        let root_url = self.root_url()?;
        let existing = self
            .index
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default();

        // BFS directory walk with bounded concurrency.
        let mut visited: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<Url> = VecDeque::new();
        visited.insert(canonical_url_key(&root_url));
        queue.push_back(root_url.clone());

        let mut dir_files: Vec<(Url, Vec<client::DavEntry>)> = Vec::new();
        let mut dirs_scanned = 0usize;

        while !queue.is_empty() && dirs_scanned < MAX_SCAN_DIRECTORIES {
            let mut batch = Vec::new();
            while batch.len() < MAX_SCAN_CONCURRENCY && dirs_scanned < MAX_SCAN_DIRECTORIES {
                let Some(dir) = queue.pop_front() else {
                    break;
                };
                dirs_scanned += 1;
                batch.push(dir);
            }
            let mut handles = Vec::with_capacity(batch.len());
            for dir in batch {
                let client = self.client.clone();
                handles.push(tokio::spawn(async move {
                    let result = client.propfind_depth_1(&dir).await;
                    (dir, result)
                }));
            }
            for handle in handles {
                let (dir, result) = handle
                    .await
                    .map_err(|error| anyhow!("WebDAV scan task failed: {error}"))?;
                let entries = match result {
                    Ok(entries) => entries,
                    Err(error) if dir == root_url => return Err(error),
                    Err(error) => {
                        log::warn!("WebDAV PROPFIND failed for {}: {error:#}", dir.path());
                        continue;
                    }
                };
                let mut files = Vec::new();
                for entry in entries {
                    if entry.is_collection {
                        let key = canonical_url_key(&entry.url);
                        if visited.insert(key) {
                            queue.push_back(entry.url);
                        }
                    } else {
                        files.push(entry);
                    }
                }
                dir_files.push((dir, files));
            }
        }

        let mut tracks = Vec::new();
        let mut summary = WebDavScanSummary::default();
        let mut tag_peeks = 0usize;

        for (_dir_url, files) in dir_files {
            // Index sibling cover images and .lrc files within this directory.
            let mut folder_cover: Option<Url> = None;
            let mut best_cover_rank = usize::MAX;
            let mut lrc_by_stem: HashMap<String, Url> = HashMap::new();

            for file in &files {
                let lower_name = file.file_name.to_ascii_lowercase();
                if let Some(rank) = index::folder_cover_rank(&lower_name) {
                    if rank < best_cover_rank {
                        best_cover_rank = rank;
                        folder_cover = Some(file.url.clone());
                    }
                }
                if let Some(stem) = lower_name
                    .strip_suffix(".lrc")
                    .or_else(|| lower_name.strip_suffix(".txt"))
                {
                    lrc_by_stem
                        .entry(stem.to_string())
                        .or_insert_with(|| file.url.clone());
                }
            }

            for file in files {
                if !index::is_supported_audio_name(&file.file_name) {
                    continue;
                }
                let url_str = file.url.to_string();
                let stem_lower = file
                    .file_name
                    .rsplit_once('.')
                    .map(|(stem, _)| stem.to_ascii_lowercase())
                    .unwrap_or_default();
                let lrc_url = lrc_by_stem.get(&stem_lower).map(Url::to_string);
                let cover_url = folder_cover.as_ref().map(Url::to_string);

                if let Some(prev) = existing.existing_track(&url_str) {
                    if prev.matches_remote(
                        file.etag.as_deref(),
                        file.content_length,
                        file.last_modified.as_deref(),
                    ) {
                        let mut reused = prev.clone();
                        reused.song.source = Some(self.id.clone());
                        reused.lrc_url = lrc_url;
                        reused.cover_url = cover_url.clone();
                        if let Some(ref c_url) = cover_url {
                            reused.song.cover_art = Some(index::stable_id("wdcover", c_url));
                        }
                        tracks.push(reused);
                        summary.reused_tracks += 1;
                        continue;
                    }
                }

                // Partial HTTP Range tag peek (bounded per scan so massive libraries stay fast).
                let peeked_tags = if tag_peeks < MAX_TAG_PEEKS_PER_SCAN {
                    tag_peeks += 1;
                    self.client.peek_audio_tags(&file.url, &file.file_name).await
                } else {
                    None
                };

                let track = infer_metadata_from_path(
                    &self.id,
                    &file,
                    &self.root_path,
                    cover_url,
                    lrc_url,
                    peeked_tags,
                );
                tracks.push(track);
                summary.parsed_tracks += 1;
            }
        }

        summary.total_tracks = tracks.len();
        let updated = WebDavLibraryIndex::from_tracks(tracks);
        if let Err(error) = updated.save(&self.data_dir.join("library-index.json")) {
            log::warn!(
                "failed to save WebDAV library index for {}: {error:#}",
                self.id.profile_id
            );
        }
        if let Ok(mut guard) = self.index.write() {
            *guard = updated;
        }
        Ok(summary)
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
            .map_err(|_| anyhow!("WebDAV favorites lock poisoned"))?;
        guard.set(key, starred);
        guard.save(&path)
    }

    pub fn search(&self, query: &str, songs: u32, albums: u32, artists: u32) -> SearchResults {
        self.index
            .read()
            .map(|guard| guard.search(query, songs as usize, albums as usize, artists as usize))
            .unwrap_or_default()
    }

    pub async fn lyrics(&self, song: &Song) -> Result<Lyrics> {
        let lrc_url = self
            .index
            .read()
            .ok()
            .and_then(|guard| guard.lrc_url(&song.id).map(str::to_string));
        let Some(url) = lrc_url else {
            return Ok(Lyrics::default());
        };
        let bytes = self.client.get_bytes(&url).await?;
        let text = String::from_utf8_lossy(&bytes);
        let synced = crate::lyrics::lrc::parse(&text);
        if !synced.lines.is_empty() {
            Ok(synced)
        } else {
            Ok(crate::lyrics::lrc::plain(&text))
        }
    }

    pub fn resolve_playback(&self, song: &Song) -> Result<PlaybackResource> {
        let file_url = self
            .index
            .read()
            .ok()
            .and_then(|guard| guard.file_url(&song.id).map(str::to_string))
            .or_else(|| {
                song.path
                    .as_deref()
                    .filter(|p| p.starts_with("http://") || p.starts_with("https://"))
                    .map(str::to_string)
            })
            .ok_or_else(|| anyhow!("WebDAV file URL not found for track {}", song.id))?;
        let mime_hint = song.content_type.clone().or_else(|| {
            song.suffix
                .as_deref()
                .and_then(|ext| crate::source::local::mime_for_path(std::path::Path::new(&format!("f.{ext}"))))
                .map(str::to_string)
        });
        let http = HttpResource::new(file_url)
            .with_headers(self.auth_headers())
            .with_route(self.route.clone())
            .with_mime_hint(mime_hint);
        let duration = song
            .duration
            .and_then(|secs| u64::try_from(secs).ok())
            .filter(|secs| *secs > 0)
            .map(Duration::from_secs);
        Ok(PlaybackResource {
            media: MediaKey::new(self.id.clone(), &song.id),
            representation: "webdav".to_string(),
            http,
            backups: Vec::new(),
            quality: AudioQuality {
                codec: song.suffix.as_deref().map(|s| s.to_ascii_uppercase()),
                bandwidth: song
                    .bit_rate
                    .and_then(|b| u64::try_from(b).ok())
                    .map(|b| b.saturating_mul(1_000)),
                sample_rate: None,
                channels: None,
            },
            duration,
        })
    }

    pub fn cover_url(&self, id: &str) -> Result<String> {
        if id.trim().is_empty() {
            anyhow::bail!("empty WebDAV cover id");
        }
        Ok(format!("webdav-cover://{id}"))
    }

    pub async fn cover_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let cover_id = url.strip_prefix("webdav-cover://").unwrap_or(url);
        let remote_url = self
            .index
            .read()
            .ok()
            .and_then(|guard| guard.cover_url(cover_id).map(str::to_string))
            .ok_or_else(|| anyhow!("WebDAV cover not found: {cover_id}"))?;
        self.client.get_bytes(&remote_url).await
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WebDavScanSummary {
    pub total_tracks: usize,
    pub parsed_tracks: usize,
    pub reused_tracks: usize,
}

pub fn normalize_webdav_url(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("Enter a WebDAV server URL");
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let parsed = Url::parse(&with_scheme).map_err(|e| anyhow!("Invalid WebDAV URL: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        anyhow::bail!("WebDAV URL must use http:// or https://");
    }
    if parsed.host_str().is_none() {
        anyhow::bail!("WebDAV URL is missing a host name");
    }
    Ok(parsed.as_str().trim_end_matches('/').to_string())
}

pub fn normalize_root_path(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return "/".to_string();
    }
    let cleaned = trimmed.trim_matches('/');
    format!("/{cleaned}")
}

fn canonical_url_key(url: &Url) -> String {
    url.as_str().trim_end_matches('/').to_string()
}

/// Keyring target identifier for storing WebDAV passwords safely alongside server passwords.
pub fn webdav_credential_key(profile_id: &str) -> String {
    format!("webdav-{profile_id}")
}

pub fn resolve_webdav_password(profile_id: &str, fallback_password: &str, in_keyring: bool) -> String {
    if in_keyring {
        if let Some((_, password)) = crate::secrets::load_password(&webdav_credential_key(profile_id)) {
            return password;
        }
    }
    fallback_password.to_string()
}
