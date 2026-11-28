//! Background download task for offline-pinned tracks.
//!
//! Reuses completed audio cache files without re-downloading when present, otherwise
//! streams the resolved audio resource into `<offline_dir>/<hash>.<ext>` atomically,
//! and pre-caches cover art so the track is completely self-contained offline.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use md5::{Digest, Md5};

use super::store::{now_unix_secs, offline_file_name, OfflineEntry};
use crate::models::{Config, Song, TranscodingQuality};
use crate::source::{MediaKey, MusicSource, ProviderKind};

#[derive(Debug)]
pub(crate) struct OfflineDownloadOutcome {
    pub(crate) media: MediaKey,
    pub(crate) result: Result<OfflineEntry, String>,
}

/// Download (or promote from audio cache) one track into `offline_dir`, plus cache its cover.
pub(crate) async fn download_track(
    api: MusicSource,
    song: Song,
    quality: TranscodingQuality,
    config: Config,
    offline_dir: PathBuf,
    audio_cache_dir: PathBuf,
) -> OfflineDownloadOutcome {
    let media = MediaKey::new(api.id().clone(), &song.id);
    let result = download_track_inner(
        &api,
        &song,
        &media,
        quality,
        &config,
        &offline_dir,
        &audio_cache_dir,
    )
    .await
    .map_err(|error| format!("{error:#}"));
    OfflineDownloadOutcome { media, result }
}

async fn download_track_inner(
    api: &MusicSource,
    song: &Song,
    media: &MediaKey,
    quality: TranscodingQuality,
    config: &Config,
    offline_dir: &Path,
    audio_cache_dir: &Path,
) -> Result<OfflineEntry> {
    if api.id().provider == ProviderKind::LocalFolder {
        anyhow::bail!("Local folder tracks are already stored on disk");
    }
    fs::create_dir_all(offline_dir)
        .with_context(|| format!("failed to create offline folder {}", offline_dir.display()))?;

    let resource = api.resolve_playback(song, quality).await?;
    let effective_quality = if api.capabilities().transcoding {
        TranscodingQuality::effective_for(
            quality,
            song.suffix.as_deref(),
            song.content_type.as_deref(),
        )
    } else {
        TranscodingQuality::Original
    };
    let suffix = if effective_quality.max_bit_rate().is_some() {
        Some("mp3")
    } else {
        song.suffix.as_deref()
    };
    let file_name = offline_file_name(media, suffix);
    let dest_path = offline_dir.join(&file_name);

    // 1. Reuse completed passive audio cache if available (zero network cost).
    let cache_key = resource.cache_key();
    let cached_audio = passive_cache_path(audio_cache_dir, &cache_key);
    let bytes = if cached_audio.is_file() {
        promote_or_copy(&cached_audio, &dest_path)?
    } else {
        // 2. Download over HTTP via the resource's configured proxy/headers.
        let part_path = offline_dir.join(format!("{file_name}.part"));
        let downloaded = stream_to_file(&resource.http, &part_path).await;
        match downloaded {
            Ok(len) => {
                if dest_path.exists() {
                    let _ = fs::remove_file(&dest_path);
                }
                fs::rename(&part_path, &dest_path).with_context(|| {
                    format!(
                        "failed to finalize offline file {}",
                        dest_path.display()
                    )
                })?;
                len
            }
            Err(error) => {
                let _ = fs::remove_file(&part_path);
                return Err(error);
            }
        }
    };

    // 3. Best-effort pre-cache cover artwork so offline browsing shows album art.
    if let Some(cover_id) = song.cover_art.as_deref() {
        if crate::config::read_source_cover_cache(config, api.id(), cover_id).is_none() {
            if let Ok(cover_url) = api.cover_url(cover_id, 384) {
                if let Ok(cover_bytes) = api.get_bytes(&cover_url).await {
                    let normalized = crate::app::covers::artwork::normalize_cover_bytes(cover_bytes);
                    crate::config::write_source_cover_cache(config, api.id(), cover_id, &normalized);
                }
            }
        }
    }

    let mut stored_song = song.clone();
    stored_song.source = Some(api.id().clone());
    Ok(OfflineEntry {
        media: media.clone(),
        song: stored_song,
        file_name,
        quality: effective_quality,
        bytes,
        pinned_at: now_unix_secs(),
    })
}

async fn stream_to_file(resource: &crate::source::HttpResource, part_path: &Path) -> Result<u64> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(600));
    match resource.route() {
        crate::models::network::NetworkRoute::System => {}
        crate::models::network::NetworkRoute::Direct => {
            builder = builder.no_proxy();
        }
        crate::models::network::NetworkRoute::Proxy(proxy_url) => {
            builder = builder.proxy(
                reqwest::Proxy::all(proxy_url)
                    .map_err(|e| anyhow!("invalid proxy for offline download: {e}"))?,
            );
        }
    }
    let client = builder.build().context("failed to build download client")?;
    let mut response = client
        .get(resource.url())
        .headers(resource.headers().clone())
        .send()
        .await
        .map_err(|error| anyhow!("offline download failed: {}", error.without_url()))?;
    if !response.status().is_success() {
        anyhow::bail!("offline download returned HTTP {}", response.status());
    }
    let mut file = File::create(part_path)
        .with_context(|| format!("failed to create {}", part_path.display()))?;
    let mut total: u64 = 0;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| anyhow!("download read failed: {}", error.without_url()))?
    {
        file.write_all(&chunk)?;
        total = total.saturating_add(chunk.len() as u64);
    }
    file.flush()?;
    if total == 0 {
        anyhow::bail!("downloaded audio file is empty");
    }
    Ok(total)
}

fn promote_or_copy(src: &Path, dst: &Path) -> Result<u64> {
    if dst.exists() {
        let _ = fs::remove_file(dst);
    }
    let len = fs::metadata(src)?.len();
    if fs::hard_link(src, dst).is_ok() {
        return Ok(len);
    }
    fs::copy(src, dst).with_context(|| {
        format!(
            "failed to copy cached audio {} to {}",
            src.display(),
            dst.display()
        )
    })?;
    Ok(len)
}

fn passive_cache_path(cache_dir: &Path, cache_key: &str) -> PathBuf {
    let mut hasher = Md5::new();
    hasher.update(cache_key.as_bytes());
    let name = format!("{:x}", hasher.finalize());
    cache_dir.join(format!("{name}.audio"))
}
