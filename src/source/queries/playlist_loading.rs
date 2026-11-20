//! Stream provider playlist results through one bounded UI update channel.

use anyhow::Result;
use tokio::sync::mpsc::Sender;

use super::MusicSource;
use crate::playlist::loading::{
    PlaylistFailureKind, PlaylistLoadError, PlaylistProgress, PlaylistUpdate,
};
use crate::source::bilibili::client::BilibiliError;

mod opensubsonic;

impl MusicSource {
    pub(crate) async fn load_playlist(&self, id: &str, updates: Sender<PlaylistUpdate>) {
        let result: Result<PlaylistProgress> = async {
            match self {
                Self::Bilibili(provider) => {
                    let id = id.parse::<u64>()?;
                    let summary = provider.load_favorite_tracks(id, &updates).await?;
                    Ok(summary.progress)
                }
                Self::OpenSubsonic(adapter) => {
                    opensubsonic::load(adapter.provider(), self.id().clone(), id, updates.clone())
                        .await
                }
            }
        }
        .await;
        let update = match result {
            Ok(progress) => PlaylistUpdate::Complete(progress),
            Err(error) => PlaylistUpdate::Failed(playlist_load_error(error)),
        };
        // Cancellation closes the bounded receiver. No terminal message from a
        // cancelled or replaced request is forwarded through the main mailbox.
        let _ = updates.send(update).await;
    }
}

fn playlist_load_error(error: anyhow::Error) -> PlaylistLoadError {
    let kind = match error.downcast_ref::<BilibiliError>() {
        Some(
            BilibiliError::NotAuthenticated
            | BilibiliError::AccountMismatch
            | BilibiliError::PermissionDenied,
        ) => PlaylistFailureKind::Authorization,
        Some(BilibiliError::Network | BilibiliError::Timeout) => PlaylistFailureKind::Network,
        Some(BilibiliError::InvalidResponse) => PlaylistFailureKind::InvalidResponse,
        _ => PlaylistFailureKind::Other,
    };
    PlaylistLoadError {
        kind,
        message: error.to_string(),
    }
}
