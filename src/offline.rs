//! Protected offline audio library (`offline/`), manifest index, and download jobs.
//!
//! Offline-pinned tracks live under `<audio_cache_dir>/offline/` with a JSON manifest
//! (`offline-manifest.json`), completely protected from automatic LRU audio cache eviction.
//! When offline mode is enabled, library browsing, search, favorites, playlists, and
//! playback resolve directly from this store without network requests.

mod download;
mod outbox;
mod store;

#[cfg(test)]
mod tests;

pub(crate) use download::{download_track, OfflineDownloadOutcome};
pub(crate) use outbox::{OfflineOutbox, PendingSyncOp};
pub(crate) use store::{OfflineStats, OfflineStore, OfflineTrackStatus};
