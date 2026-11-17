//! Adapt complete server responses and progressive favorites to one UI stream.

use anyhow::Result;
use tokio::sync::mpsc::Sender;

use super::MusicSource;
use crate::playlist::loading::{
    PlaylistFailureKind, PlaylistLoadError, PlaylistProgress, PlaylistUpdate, PLAYLIST_BATCH_SIZE,
};
use crate::source::bilibili::client::BilibiliError;

impl MusicSource {
    pub(crate) async fn load_playlist(&self, id: &str, updates: Sender<PlaylistUpdate>) {
        let result: Result<PlaylistProgress> = async {
            match self {
                Self::Bilibili(provider) => {
                    let id = id.parse::<u64>()?;
                    let summary = provider.load_favorite_tracks(id, &updates).await?;
                    Ok(summary.progress)
                }
                Self::OpenSubsonic(_) => {
                    let details = self.playlist(id).await?;
                    let mut progress = PlaylistProgress {
                        unavailable_count: details.unavailable_count,
                        ..Default::default()
                    };
                    updates
                        .send(PlaylistUpdate::Metadata(details.playlist))
                        .await?;
                    let mut songs = Vec::with_capacity(PLAYLIST_BATCH_SIZE);
                    for song in details.songs {
                        songs.push(song);
                        progress.songs += 1;
                        if songs.len() == PLAYLIST_BATCH_SIZE {
                            updates
                                .send(PlaylistUpdate::Batch {
                                    songs: std::mem::replace(
                                        &mut songs,
                                        Vec::with_capacity(PLAYLIST_BATCH_SIZE),
                                    ),
                                    progress,
                                })
                                .await?;
                        }
                    }
                    if !songs.is_empty() {
                        updates
                            .send(PlaylistUpdate::Batch { songs, progress })
                            .await?;
                    }
                    Ok(progress)
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
