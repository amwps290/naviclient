//! First-result publication and timed batching without an unbounded UI queue.

use std::time::Instant;
use tokio::sync::mpsc::Sender;
use tokio::time::{Duration, Instant as FlushInstant};

use crate::models::Song;
use crate::playlist::loading::{PlaylistProgress, PlaylistUpdate, PLAYLIST_BATCH_SIZE};

use super::super::client::BilibiliError;

const FLUSH_INTERVAL: Duration = Duration::from_millis(100);

pub(super) struct TrackUpdates<'a> {
    sender: &'a Sender<PlaylistUpdate>,
    songs: Vec<Song>,
    pub(super) progress: PlaylistProgress,
    published: bool,
    deadline: Option<FlushInstant>,
    started: Instant,
}

impl<'a> TrackUpdates<'a> {
    pub(super) fn new(sender: &'a Sender<PlaylistUpdate>, started: Instant) -> Self {
        Self {
            sender,
            songs: Vec::with_capacity(PLAYLIST_BATCH_SIZE),
            progress: PlaylistProgress::default(),
            published: false,
            deadline: None,
            started,
        }
    }

    pub(super) fn deadline(&self) -> Option<FlushInstant> {
        self.deadline
    }

    pub(super) async fn push(&mut self, song: Song) -> Result<(), BilibiliError> {
        self.songs.push(song);
        self.progress.songs += 1;
        self.deadline
            .get_or_insert_with(|| FlushInstant::now() + FLUSH_INTERVAL);
        if self.songs.len() == PLAYLIST_BATCH_SIZE {
            self.flush().await?;
        }
        Ok(())
    }

    pub(super) async fn flush_first(&mut self) -> Result<(), BilibiliError> {
        if !self.published {
            self.flush().await?;
        }
        Ok(())
    }

    pub(super) async fn flush(&mut self) -> Result<(), BilibiliError> {
        if self.songs.is_empty() {
            return Ok(());
        }
        let songs = std::mem::replace(&mut self.songs, Vec::with_capacity(PLAYLIST_BATCH_SIZE));
        let count = songs.len();
        self.send(PlaylistUpdate::Batch {
            songs,
            progress: self.progress,
        })
        .await?;
        if !self.published {
            log::debug!(
                "bilibili favorites first batch; elapsed_ms={} songs={} pages={}",
                self.started.elapsed().as_millis(),
                count,
                self.progress.pages,
            );
            self.published = true;
        }
        self.deadline = None;
        Ok(())
    }

    pub(super) async fn report_progress(&mut self) -> Result<(), BilibiliError> {
        self.flush().await?;
        self.send(PlaylistUpdate::Progress(self.progress)).await
    }

    pub(super) async fn send(&self, update: PlaylistUpdate) -> Result<(), BilibiliError> {
        self.sender
            .send(update)
            .await
            .map_err(|_| BilibiliError::Network)
    }
}
