//! Disk-cache ownership and publication, independent of HTTP response cursors.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};
use md5::{Digest, Md5};

use super::stream_control::StreamControl;
use crate::source::CachePolicy;

const MAX_AUDIO_CACHE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

pub(super) fn prepare_cache_dir(cache_dir: &Path) -> Result<()> {
    prune_to(cache_dir, MAX_AUDIO_CACHE_BYTES)
}

fn prune_to(cache_dir: &Path, limit: u64) -> Result<()> {
    fs::create_dir_all(cache_dir).context("failed to create audio cache directory")?;

    let mut entries = fs::read_dir(cache_dir)
        .context("failed to read audio cache directory")?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            metadata.is_file().then(|| {
                (
                    entry.path(),
                    metadata.len(),
                    metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                )
            })
        })
        .collect::<Vec<_>>();
    let mut total = entries.iter().map(|(_, len, _)| len).sum::<u64>();
    if total <= limit {
        return Ok(());
    }

    entries.sort_by_key(|(_, _, modified)| *modified);
    for (path, len, _) in entries {
        if total <= limit {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
    Ok(())
}

pub(super) fn cache_paths(cache_dir: &Path, cache_key: &str) -> (PathBuf, PathBuf) {
    let mut hasher = Md5::new();
    hasher.update(cache_key.as_bytes());
    let name = format!("{:x}", hasher.finalize());
    (
        cache_dir.join(format!("{name}.audio")),
        cache_dir.join(format!("{name}.part")),
    )
}

pub(super) struct BufferProgress {
    cached_bytes: AtomicU64,
    total_bytes: AtomicU64,
}

impl BufferProgress {
    pub(super) fn new(cached_bytes: u64, total_bytes: Option<u64>) -> Self {
        Self {
            cached_bytes: AtomicU64::new(cached_bytes),
            total_bytes: AtomicU64::new(total_bytes.unwrap_or(0)),
        }
    }

    pub(super) fn cached_bytes(&self) -> u64 {
        self.cached_bytes.load(Ordering::Relaxed)
    }

    pub(super) fn total_bytes(&self) -> u64 {
        self.total_bytes.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(super) fn set_cached_bytes_for_test(&self, cached: u64) {
        self.cached_bytes.store(cached, Ordering::Relaxed);
    }

    pub(super) fn buffered_duration(&self, duration: Duration) -> Duration {
        let total = self.total_bytes();
        if total == 0 {
            return Duration::ZERO;
        }
        let cached = self.cached_bytes().min(total);
        duration.mul_f64(cached as f64 / total as f64)
    }
}

pub(super) struct StreamCache {
    control: Arc<StreamControl>,
    file: Option<File>,
    complete_path: PathBuf,
    partial_path: PathBuf,
    cached_len: u64,
    complete: bool,
    logged_bucket: u64,
    progress: Arc<BufferProgress>,
    temporary_limit: Option<u64>,
}

impl StreamCache {
    /// Probe completed files before any network request. Partial files are
    /// opened only after a successful response supplies its resume policy.
    #[cfg(test)]
    pub(super) fn open(
        cache_dir: &Path,
        cache_key: &str,
        control: Arc<StreamControl>,
    ) -> Result<Self> {
        Self::open_with_policy(cache_dir, cache_key, control, CachePolicy::Persistent)
    }

    pub(super) fn open_with_policy(
        cache_dir: &Path,
        cache_key: &str,
        control: Arc<StreamControl>,
        policy: CachePolicy,
    ) -> Result<Self> {
        if let CachePolicy::Temporary { max_bytes } = policy {
            let file = if max_bytes == 0 {
                None
            } else {
                tempfile::tempfile()
                    .map_err(|error| {
                        log::warn!(
                            "temporary audio cache is unavailable, continuing without it: {error}"
                        );
                        error
                    })
                    .ok()
            };
            return Ok(Self {
                control,
                file,
                complete_path: PathBuf::new(),
                partial_path: PathBuf::new(),
                cached_len: 0,
                complete: false,
                logged_bucket: 0,
                progress: Arc::new(BufferProgress::new(0, None)),
                temporary_limit: Some(max_bytes),
            });
        }
        let (complete_path, mut partial_path) = cache_paths(cache_dir, cache_key);
        let file = OpenOptions::new().read(true).open(&complete_path).ok();
        let cached_len = match file.as_ref() {
            Some(file) => file.metadata()?.len(),
            None => 0,
        };
        let complete = file.is_some();
        if complete {
            log::info!(
                "audio cache hit; file={} bytes={cached_len}",
                complete_path.display()
            );
        } else if control.is_speculative() {
            // A duplicate queue entry must not overwrite the active reader's
            // partial file, or another speculative reader's temporary file.
            static NEXT_PART: AtomicU64 = AtomicU64::new(0);
            let serial = NEXT_PART.fetch_add(1, Ordering::Relaxed);
            partial_path.set_extension(format!("preload-{}-{serial}.part", std::process::id()));
        }
        Ok(Self {
            control,
            file,
            complete_path,
            partial_path,
            cached_len,
            complete,
            logged_bucket: if complete { 10 } else { 0 },
            progress: Arc::new(BufferProgress::new(
                cached_len,
                complete.then_some(cached_len),
            )),
            temporary_limit: None,
        })
    }

    pub(super) fn complete(&self) -> bool {
        self.complete
    }

    pub(super) fn cached_len(&self) -> u64 {
        self.cached_len
    }

    pub(super) fn progress(&self) -> Arc<BufferProgress> {
        self.progress.clone()
    }

    pub(super) fn prepare_partial(&mut self, len: Option<u64>, range_supported: bool) {
        debug_assert!(!self.complete);
        self.progress
            .total_bytes
            .store(len.unwrap_or(0), Ordering::Relaxed);
        if self.temporary_limit.is_some() {
            return;
        }
        let cache_dir = self.partial_path.parent().unwrap_or_else(|| Path::new("."));
        self.file = fs::create_dir_all(cache_dir)
            .and_then(|()| {
                OpenOptions::new()
                    .create(true)
                    .read(true)
                    .write(true)
                    .truncate(false)
                    .open(&self.partial_path)
            })
            .map_err(|error| {
                log::warn!("audio cache is unavailable, continuing without it: {error}");
                error
            })
            .ok();
        self.cached_len = self
            .file
            .as_ref()
            .and_then(|file| file.metadata().ok())
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        if !range_supported || len.is_some_and(|len| self.cached_len > len) {
            if self.cached_len > 0 {
                log::debug!(
                    "discarding partial audio cache; cached_bytes={} range_supported={range_supported} total_bytes={len:?}",
                    self.cached_len
                );
            }
            self.discard_partial();
        }
        self.progress
            .cached_bytes
            .store(self.cached_len, Ordering::Relaxed);
        self.logged_bucket = len
            .filter(|len| *len > 0)
            .map(|len| self.cached_len.saturating_mul(10) / len)
            .unwrap_or(0);
    }

    fn disable(&mut self) {
        self.file = None;
        self.cached_len = 0;
        self.progress.cached_bytes.store(0, Ordering::Relaxed);
    }

    fn discard_partial(&mut self) {
        if let Some(file) = self.file.as_ref() {
            if let Err(error) = file.set_len(0) {
                log::warn!("failed to reset partial audio cache, continuing without it: {error}");
                self.disable();
                return;
            }
        }
        self.cached_len = 0;
        self.progress.cached_bytes.store(0, Ordering::Relaxed);
    }

    /// None tells the caller to read the network at the same byte position.
    /// Completed-cache failures remain errors because there is no open response.
    pub(super) fn read(&mut self, position: u64, buffer: &mut [u8]) -> io::Result<Option<usize>> {
        if position >= self.cached_len {
            return Ok(None);
        }
        let available = self.cached_len - position;
        let read_len = usize::try_from(available.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let result = (|| {
            let file = self
                .file
                .as_mut()
                .ok_or_else(|| io::Error::other("audio cache file is closed"))?;
            file.seek(SeekFrom::Start(position))?;
            let read = file.read(&mut buffer[..read_len])?;
            if read == 0 && read_len > 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "audio cache file was shortened",
                ));
            }
            Ok(read)
        })();
        match result {
            Ok(read) => Ok(Some(read)),
            Err(error) if !self.complete => {
                log::warn!("failed to read partial audio cache, continuing without it: {error}");
                self.disable();
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }

    /// Only a contiguous prefix may be published as a complete cache file.
    pub(super) fn append(&mut self, start: u64, bytes: &[u8]) {
        if self.complete || start != self.cached_len || bytes.is_empty() {
            return;
        }
        let bytes = if let Some(limit) = self.temporary_limit {
            let remaining = limit.saturating_sub(self.cached_len);
            &bytes[..bytes
                .len()
                .min(usize::try_from(remaining).unwrap_or(usize::MAX))]
        } else {
            bytes
        };
        if bytes.is_empty() {
            return;
        }
        let Some(file) = self.file.as_mut() else {
            return;
        };
        if let Err(error) = file
            .seek(SeekFrom::Start(start))
            .and_then(|_| file.write_all(bytes))
        {
            log::warn!("failed to write audio cache, continuing without it: {error}");
            self.disable();
            return;
        }
        self.cached_len = self.cached_len.saturating_add(bytes.len() as u64);
        self.progress
            .cached_bytes
            .store(self.cached_len, Ordering::Relaxed);
        let total = self.progress.total_bytes.load(Ordering::Relaxed);
        if let Some(ratio) = self.cached_len.saturating_mul(10).checked_div(total) {
            let bucket = ratio.min(10);
            if bucket > self.logged_bucket {
                self.logged_bucket = bucket;
                log::info!(
                    "audio cache progress; cached_bytes={} total_bytes={total} percent={}",
                    self.cached_len,
                    bucket * 10
                );
            }
        }
    }

    pub(super) fn finish_estimated_stream(&mut self, actual_len: u64) {
        self.progress
            .total_bytes
            .store(actual_len, Ordering::Relaxed);
        self.logged_bucket = 10;
        log::info!(
            "audio cache progress; cached_bytes={} total_bytes={actual_len} percent=100",
            self.cached_len
        );
    }
}

impl Drop for StreamCache {
    fn drop(&mut self) {
        if self.temporary_limit.is_some() {
            // tempfile() owns an anonymous/private file; closing it releases
            // the bytes and never publishes a persistent .audio/.part path.
            self.file.take();
            return;
        }
        if self.complete {
            return;
        }
        // Close the file before renaming or deleting it, including on Windows.
        self.file.take();
        let total = self.progress.total_bytes.load(Ordering::Relaxed);
        let fully_cached = total > 0 && self.cached_len >= total;
        let unverified_preload = self.control.is_speculative() && !self.control.fully_decoded();
        if !fully_cached || unverified_preload {
            if self.control.is_speculative() {
                let _ = fs::remove_file(&self.partial_path);
            }
            return;
        }
        if let Err(error) = fs::rename(&self.partial_path, &self.complete_path) {
            log::warn!("failed to finalize audio cache: {error}");
        } else {
            log::info!(
                "audio cache finalized; file={} bytes={}",
                self.complete_path.display(),
                self.cached_len
            );
        }
    }
}

#[cfg(test)]
mod tests;
