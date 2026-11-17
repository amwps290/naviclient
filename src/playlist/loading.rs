//! Bounded, ordered playlist updates shared by providers and the library UI.

use tokio::sync::mpsc;

use crate::models::{Playlist, Song};

pub(crate) const PLAYLIST_BATCH_SIZE: usize = 128;
const UPDATE_CAPACITY: usize = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PlaylistProgress {
    pub pages: usize,
    pub videos: usize,
    pub songs: usize,
    pub unavailable_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlaylistFailureKind {
    Network,
    Authorization,
    InvalidResponse,
    Other,
}

#[derive(Debug)]
pub(crate) struct PlaylistLoadError {
    pub kind: PlaylistFailureKind,
    pub message: String,
}

impl PlaylistLoadError {
    pub(crate) fn invalid_response() -> Self {
        Self {
            kind: PlaylistFailureKind::InvalidResponse,
            message: rust_i18n::t!("Playlist data is invalid. Retry loading.").to_string(),
        }
    }

    pub(crate) fn interrupted() -> Self {
        Self {
            kind: PlaylistFailureKind::Network,
            message: rust_i18n::t!("Playlist loading stopped before completion. Retry loading.")
                .to_string(),
        }
    }
}

#[derive(Debug)]
pub(crate) enum PlaylistUpdate {
    Metadata(Playlist),
    Batch {
        songs: Vec<Song>,
        progress: PlaylistProgress,
    },
    Progress(PlaylistProgress),
    Complete(PlaylistProgress),
    Failed(PlaylistLoadError),
}

pub(crate) fn playlist_update_channel(
) -> (mpsc::Sender<PlaylistUpdate>, mpsc::Receiver<PlaylistUpdate>) {
    mpsc::channel(UPDATE_CAPACITY)
}
