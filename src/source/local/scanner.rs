//! Recursive directory scanner with incremental `(mtime, size)` tag parsing.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use lofty::file::{AudioFile, TaggedFileExt};
use lofty::probe::Probe;
use lofty::tag::{Accessor, ItemKey};
use walkdir::WalkDir;

use super::index::{stable_id, CoverOrigin, IndexedTrack, LocalLibraryIndex};
use super::{mime_for_path, ScanSummary};
use crate::models::Song;
use crate::source::SourceId;

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

pub(super) fn scan_roots(
    source_id: &SourceId,
    roots: &[PathBuf],
    existing: &LocalLibraryIndex,
) -> (LocalLibraryIndex, ScanSummary) {
    let mut tracks = Vec::new();
    let mut folder_cover_cache: HashMap<PathBuf, Option<PathBuf>> = HashMap::new();
    let mut summary = ScanSummary::default();

    for root in roots {
        if !root.is_dir() {
            continue;
        }
        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            if !is_supported_audio_file(path) {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                summary.skipped_files += 1;
                continue;
            };
            let file_size = meta.len();
            let mtime_secs = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            if let Some(prev) = existing.existing_track(path) {
                if prev.mtime_secs == mtime_secs && prev.file_size == file_size {
                    let mut reused = prev.clone();
                    reused.song.source = Some(source_id.clone());
                    tracks.push(reused);
                    summary.reused_tracks += 1;
                    continue;
                }
            }

            let folder_cover = path.parent().and_then(|dir| {
                folder_cover_cache
                    .entry(dir.to_path_buf())
                    .or_insert_with(|| find_folder_cover(dir))
                    .clone()
            });

            match parse_audio_file(source_id, path, mtime_secs, file_size, folder_cover) {
                Some(track) => {
                    tracks.push(track);
                    summary.parsed_tracks += 1;
                }
                None => {
                    summary.skipped_files += 1;
                }
            }
        }
    }

    summary.total_tracks = tracks.len();
    (LocalLibraryIndex::from_tracks(tracks), summary)
}

pub(super) fn is_supported_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .is_some_and(|ext| SUPPORTED_EXTENSIONS.contains(&ext.as_str()))
}

pub(super) fn extract_embedded_cover(path: &Path) -> Option<Vec<u8>> {
    let tagged = Probe::open(path).ok()?.read().ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    tag.pictures()
        .first()
        .map(|picture| picture.data().to_vec())
}

fn find_folder_cover(dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    let mut best: Option<(usize, PathBuf)> = None;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if let Some(rank) = FOLDER_COVER_NAMES.iter().position(|&cand| cand == lower) {
            if best.as_ref().is_none_or(|(best_rank, _)| rank < *best_rank) {
                best = Some((rank, path));
            }
        }
    }
    best.map(|(_, path)| path)
}

fn parse_audio_file(
    source_id: &SourceId,
    path: &Path,
    mtime_secs: u64,
    file_size: u64,
    folder_cover: Option<PathBuf>,
) -> Option<IndexedTrack> {
    let suffix = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase());
    let fallback_title = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::trim)
        .filter(|stem| !stem.is_empty())
        .unwrap_or("Unknown Track")
        .to_string();
    let fallback_album = path
        .parent()
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("Unknown Album")
        .to_string();

    let tagged = Probe::open(path).ok().and_then(|probe| probe.read().ok());

    let mut title = fallback_title;
    let mut artist = String::from("Unknown Artist");
    let mut album_artist = String::new();
    let mut album = fallback_album;
    let mut genre = None;
    let mut track = None;
    let mut disc = None;
    let mut year = None;
    let mut duration = None;
    let mut bit_rate = None;
    let mut rg_track_gain = None;
    let mut rg_track_peak = None;
    let mut rg_album_gain = None;
    let mut rg_album_peak = None;
    let mut has_embedded_cover = false;

    if let Some(tagged) = tagged {
        let props = tagged.properties();
        let secs = props.duration().as_secs();
        if secs > 0 {
            duration = i32::try_from(secs).ok();
        }
        bit_rate = props
            .audio_bitrate()
            .or_else(|| props.overall_bitrate())
            .and_then(|kbps| i32::try_from(kbps).ok())
            .filter(|kbps| *kbps > 0);

        if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
            if let Some(val) = tag.title().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
                title = val;
            }
            if let Some(val) = tag.artist().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
                artist = val;
            }
            if let Some(val) = tag
                .get_string(ItemKey::AlbumArtist)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                album_artist = val.to_string();
            }
            if let Some(val) = tag.album().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
                album = val;
            }
            genre = tag
                .genre()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            track = tag.track().and_then(|n| i32::try_from(n).ok()).filter(|n| *n > 0);
            disc = tag.disk().and_then(|n| i32::try_from(n).ok()).filter(|n| *n > 0);
            year = tag.date().map(|d| d.year as i32).filter(|y| *y > 0);
            rg_track_gain = tag
                .get_string(ItemKey::ReplayGainTrackGain)
                .and_then(parse_replaygain_db);
            rg_track_peak = tag
                .get_string(ItemKey::ReplayGainTrackPeak)
                .and_then(parse_replaygain_float);
            rg_album_gain = tag
                .get_string(ItemKey::ReplayGainAlbumGain)
                .and_then(parse_replaygain_db);
            rg_album_peak = tag
                .get_string(ItemKey::ReplayGainAlbumPeak)
                .and_then(parse_replaygain_float);
            has_embedded_cover = !tag.pictures().is_empty();
        }
    }

    if album_artist.is_empty() {
        album_artist.clone_from(&artist);
    }

    let path_str = path.to_string_lossy().to_string();
    let song_id = stable_id("track", &path_str);
    let artist_id = stable_id("artist", &album_artist.to_lowercase());
    let album_id = stable_id("album", &format!("{artist_id}\0{}", album.to_lowercase()));

    let (cover_art, cover_origin) = if let Some(cover_path) = folder_cover {
        let key = stable_id("cover-file", &cover_path.to_string_lossy());
        (Some(key), Some(CoverOrigin::ImageFile(cover_path)))
    } else if has_embedded_cover {
        let key = stable_id("cover-emb", &format!("{album_id}\0{mtime_secs}"));
        (Some(key), Some(CoverOrigin::EmbeddedAudio(path.to_path_buf())))
    } else {
        (None, None)
    };

    let song = Song {
        source: Some(source_id.clone()),
        id: song_id,
        title,
        bilibili_part: None,
        album,
        album_id: Some(album_id),
        artist,
        artist_id: Some(artist_id),
        genre,
        track,
        disc,
        duration,
        cover_art,
        suffix,
        content_type: mime_for_path(path).map(str::to_string),
        bit_rate,
        size: i64::try_from(file_size).ok(),
        path: Some(path_str),
        replay_gain_track_gain: rg_track_gain,
        replay_gain_track_peak: rg_track_peak,
        replay_gain_album_gain: rg_album_gain,
        replay_gain_album_peak: rg_album_peak,
    };

    Some(IndexedTrack {
        file_path: path.to_path_buf(),
        mtime_secs,
        file_size,
        year,
        album_artist,
        cover_origin,
        song,
    })
}

fn parse_replaygain_db(value: &str) -> Option<f32> {
    let trimmed = value
        .trim()
        .trim_end_matches("dB")
        .trim_end_matches("db")
        .trim_end_matches("DB")
        .trim();
    trimmed.parse::<f32>().ok().filter(|v| v.is_finite())
}

fn parse_replaygain_float(value: &str) -> Option<f32> {
    value.trim().parse::<f32>().ok().filter(|v| v.is_finite())
}
