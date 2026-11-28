//! `.m3u8` / `.m3u` playlist import and export codec with NaviClient cross-source metadata.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::Accessor;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

use super::external_files_source_id;
use super::store::LocalPlaylist;
use crate::models::{BilibiliPart, Song};
use crate::source::{ProviderKind, SourceId};

const NAVICLIENT_META_PREFIX: &str = "#EXT-X-NAVICLIENT:";
const NAVICLIENT_URI_PREFIX: &str = "naviclient://";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct NaviclientTrackMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<SourceId>,
    id: String,
    title: String,
    #[serde(default)]
    artist: String,
    #[serde(default)]
    album: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    duration: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cover_art: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    suffix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bilibili_part: Option<BilibiliPart>,
}

impl From<&Song> for NaviclientTrackMeta {
    fn from(song: &Song) -> Self {
        Self {
            source: song.source.clone(),
            id: song.id.clone(),
            title: song.title.clone(),
            artist: song.artist.clone(),
            album: song.album.clone(),
            duration: song.duration,
            cover_art: song.cover_art.clone(),
            suffix: song.suffix.clone(),
            content_type: song.content_type.clone(),
            bilibili_part: song.bilibili_part.clone(),
        }
    }
}

impl NaviclientTrackMeta {
    fn into_song(self, path: Option<String>) -> Song {
        Song {
            source: self.source,
            id: self.id,
            title: self.title,
            bilibili_part: self.bilibili_part,
            album: self.album,
            artist: self.artist,
            duration: self.duration,
            cover_art: self.cover_art,
            suffix: self.suffix,
            content_type: self.content_type,
            path,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug)]
pub struct M3u8ImportResult {
    pub name: String,
    pub songs: Vec<Song>,
    pub skipped_missing: usize,
}

/// Serialize a local playlist into standard UTF-8 `.m3u8` text.
///
/// Tracks with a local file path (or an offline copy resolved via `resolve_local_path`)
/// emit their filesystem path for maximum portability with external players. Remote
/// tracks emit a `naviclient://<provider>/<profile_id>/<song_id>` URI, and all tracks
/// include `#EXT-X-NAVICLIENT:` metadata so re-importing preserves exact cross-source bindings.
pub fn export_m3u8(
    playlist: &LocalPlaylist,
    resolve_local_path: impl Fn(&Song) -> Option<PathBuf>,
) -> String {
    let mut out = String::new();
    out.push_str("#EXTM3U\n");
    let safe_name = sanitize_header_value(&playlist.name);
    if !safe_name.is_empty() {
        out.push_str(&format!("#PLAYLIST:{safe_name}\n"));
    }
    for song in &playlist.songs {
        let secs = song.duration.filter(|d| *d >= 0).unwrap_or(-1);
        let display = if song.artist.trim().is_empty() {
            sanitize_header_value(&song.title)
        } else {
            format!(
                "{} - {}",
                sanitize_header_value(&song.artist),
                sanitize_header_value(&song.title)
            )
        };
        out.push_str(&format!("#EXTINF:{secs},{display}\n"));

        let meta = NaviclientTrackMeta::from(song);
        if let Ok(json) = serde_json::to_string(&meta) {
            out.push_str(NAVICLIENT_META_PREFIX);
            out.push_str(&json);
            out.push('\n');
        }

        let location = resolve_local_path(song)
            .map(|path| path.to_string_lossy().into_owned())
            .or_else(|| {
                song.path
                    .as_deref()
                    .filter(|p| Path::new(p).is_file())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| format_naviclient_uri(song));
        out.push_str(&location);
        out.push('\n');
    }
    out
}

/// Write a local playlist to disk as UTF-8 `.m3u8`.
pub fn export_m3u8_to_file(
    playlist: &LocalPlaylist,
    path: &Path,
    resolve_local_path: impl Fn(&Song) -> Option<PathBuf>,
) -> Result<()> {
    let content = export_m3u8(playlist, resolve_local_path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
    }
    fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

/// Parse an `.m3u` or `.m3u8` file into a playlist name and playable `Song` entries.
///
/// `local_sources` maps configured `LocalFolder` source IDs to their root directories
/// so local audio paths are matched to their owning `LocalFolder` source when possible.
pub fn import_m3u8(
    path: &Path,
    local_sources: &[(SourceId, Vec<PathBuf>)],
) -> Result<M3u8ImportResult> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read playlist file {}", path.display()))?;
    let base_dir = path.parent().unwrap_or_else(|| Path::new("."));
    let fallback_name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::trim)
        .filter(|stem| !stem.is_empty())
        .unwrap_or("Imported playlist")
        .to_string();
    Ok(parse_m3u8_str(&raw, base_dir, &fallback_name, local_sources))
}

pub fn parse_m3u8_str(
    raw: &str,
    base_dir: &Path,
    fallback_name: &str,
    local_sources: &[(SourceId, Vec<PathBuf>)],
) -> M3u8ImportResult {
    let raw = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let mut name: Option<String> = None;
    let mut pending_extinf: Option<(Option<i32>, String, String)> = None;
    let mut pending_meta: Option<NaviclientTrackMeta> = None;
    let mut songs = Vec::new();
    let mut skipped_missing = 0usize;

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("#PLAYLIST:") {
            let candidate = rest.trim();
            if !candidate.is_empty() {
                name = Some(candidate.to_string());
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("#EXTINF:") {
            pending_extinf = Some(parse_extinf(rest));
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(NAVICLIENT_META_PREFIX) {
            if let Ok(meta) = serde_json::from_str::<NaviclientTrackMeta>(rest.trim()) {
                pending_meta = Some(meta);
            }
            continue;
        }
        if trimmed.starts_with('#') {
            continue;
        }

        let extinf = pending_extinf.take();
        let meta = pending_meta.take();

        if let Some(meta) = meta {
            let local_path = resolve_disk_path(trimmed, base_dir)
                .filter(|p| p.is_file())
                .map(|p| p.to_string_lossy().into_owned());
            songs.push(meta.into_song(local_path));
            continue;
        }

        if let Some((source, song_id)) = parse_naviclient_uri(trimmed) {
            let (duration, artist, title) = extinf.unwrap_or((None, String::new(), song_id.clone()));
            songs.push(Song {
                source: Some(source),
                id: song_id,
                title,
                artist,
                duration,
                ..Default::default()
            });
            continue;
        }

        if let Some(file_path) = resolve_disk_path(trimmed, base_dir).filter(|p| p.is_file()) {
            let song = song_from_local_file(&file_path, extinf, local_sources);
            songs.push(song);
            continue;
        }

        skipped_missing += 1;
    }

    M3u8ImportResult {
        name: name.unwrap_or_else(|| fallback_name.to_string()),
        songs,
        skipped_missing,
    }
}

fn sanitize_header_value(raw: &str) -> String {
    raw.replace(['\r', '\n'], " ").trim().to_string()
}

fn format_naviclient_uri(song: &Song) -> String {
    let (provider, profile_id) = match song.source.as_ref() {
        Some(source) => (provider_slug(source.provider), source.profile_id.as_str()),
        None => ("unknown", "default"),
    };
    let encoded_profile = percent_encode(profile_id);
    let encoded_id = percent_encode(&song.id);
    format!("{NAVICLIENT_URI_PREFIX}{provider}/{encoded_profile}/{encoded_id}")
}

fn parse_naviclient_uri(line: &str) -> Option<(SourceId, String)> {
    let rest = line.strip_prefix(NAVICLIENT_URI_PREFIX)?;
    let mut parts = rest.splitn(3, '/');
    let provider = parse_provider_slug(parts.next()?)?;
    let profile_id = percent_decode(parts.next()?)?;
    let song_id = percent_decode(parts.next()?)?;
    if profile_id.is_empty() || song_id.is_empty() {
        return None;
    }
    Some((SourceId::new(provider, profile_id), song_id))
}

fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        let unreserved = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~');
        if unreserved {
            out.push(*byte as char);
        } else {
            use std::fmt::Write;
            let _ = write!(&mut out, "%{byte:02X}");
        }
    }
    out
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = hex_val(bytes[i + 1])?;
            let lo = hex_val(bytes[i + 2])?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn provider_slug(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::OpenSubsonic => "opensubsonic",
        ProviderKind::Bilibili => "bilibili",
        ProviderKind::LocalFolder => "local_folder",
        ProviderKind::WebDav => "webdav",
    }
}

fn parse_provider_slug(slug: &str) -> Option<ProviderKind> {
    match slug {
        "opensubsonic" => Some(ProviderKind::OpenSubsonic),
        "bilibili" => Some(ProviderKind::Bilibili),
        "local_folder" => Some(ProviderKind::LocalFolder),
        "webdav" => Some(ProviderKind::WebDav),
        _ => None,
    }
}

fn parse_extinf(rest: &str) -> (Option<i32>, String, String) {
    let (dur_str, label) = match rest.split_once(',') {
        Some((d, l)) => (d.trim(), l.trim()),
        None => (rest.trim(), ""),
    };
    let duration = dur_str
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<f64>().ok())
        .filter(|&v| v > 0.0)
        .map(|v| v.round() as i32);
    let (artist, title) = match label.split_once(" - ") {
        Some((artist, title)) if !artist.trim().is_empty() && !title.trim().is_empty() => {
            (artist.trim().to_string(), title.trim().to_string())
        }
        _ => (String::new(), label.to_string()),
    };
    (duration, artist, title)
}

fn resolve_disk_path(entry: &str, base_dir: &Path) -> Option<PathBuf> {
    if entry.starts_with("http://") || entry.starts_with("https://") {
        return None;
    }
    if let Some(uri_path) = entry.strip_prefix("file://") {
        let decoded = percent_decode(uri_path)?;
        let raw = decoded.as_str();
        #[cfg(windows)]
        let raw = raw.strip_prefix('/').unwrap_or(raw);
        return Some(PathBuf::from(raw));
    }
    let candidate = PathBuf::from(entry);
    if candidate.is_absolute() {
        Some(candidate)
    } else {
        Some(base_dir.join(candidate))
    }
}

fn song_from_local_file(
    file_path: &Path,
    extinf: Option<(Option<i32>, String, String)>,
    local_sources: &[(SourceId, Vec<PathBuf>)],
) -> Song {
    let canonical = fs::canonicalize(file_path).unwrap_or_else(|_| file_path.to_path_buf());
    let matched_source = local_sources
        .iter()
        .find(|(_, roots)| {
            roots.iter().any(|root| {
                let root_canon = fs::canonicalize(root).unwrap_or_else(|_| root.clone());
                canonical.starts_with(&root_canon) || file_path.starts_with(root)
            })
        })
        .map(|(id, _)| id.clone())
        .unwrap_or_else(external_files_source_id);

    let fallback_title = file_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Untitled")
        .to_string();

    let (ext_dur, ext_artist, ext_title) = extinf.unwrap_or((None, String::new(), String::new()));

    let mut title = if !ext_title.is_empty() {
        ext_title
    } else {
        fallback_title
    };
    let mut artist = ext_artist;
    let mut album = String::new();
    let mut duration = ext_dur;

    if let Ok(tagged) = Probe::open(&canonical).and_then(|p| p.read()) {
        let secs = tagged.properties().duration().as_secs();
        if secs > 0 {
            duration = Some(secs.min(i32::MAX as u64) as i32);
        }
        if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
            if let Some(t) = tag.title().as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                title = t.to_string();
            }
            if let Some(a) = tag.artist().as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                artist = a.to_string();
            }
            if let Some(al) = tag.album().as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                album = al.to_string();
            }
        }
    }

    let suffix = canonical
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase());
    let path_str = canonical.to_string_lossy().into_owned();
    let id = stable_track_id(&path_str);

    Song {
        source: Some(matched_source),
        id,
        title,
        artist,
        album,
        duration,
        suffix,
        path: Some(path_str),
        ..Default::default()
    }
}

fn stable_track_id(path: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(b"track");
    hasher.update([0]);
    hasher.update(path.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(20);
    for byte in &digest[..10] {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    format!("local-track-{hex}")
}
