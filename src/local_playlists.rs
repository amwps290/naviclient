//! Cross-source local playlists persisted as JSON files, plus `.m3u8` import/export.

pub mod m3u8;
pub mod store;

#[cfg(test)]
mod tests;

use std::path::PathBuf;

use crate::source::{ProviderKind, SourceId};

pub use m3u8::import_m3u8;
pub use store::{LocalPlaylist, LocalPlaylistStore};

/// Virtual source ID used for standalone local audio files imported from `.m3u8`
/// that do not belong to any configured `LocalFolder` source yet.
pub const EXTERNAL_FILES_PROFILE_ID: &str = "m3u8-external-files";

pub fn external_files_source_id() -> SourceId {
    SourceId::new(ProviderKind::LocalFolder, EXTERNAL_FILES_PROFILE_ID)
}

pub fn default_playlists_dir() -> PathBuf {
    crate::config::config_dir().join("local-playlists")
}
