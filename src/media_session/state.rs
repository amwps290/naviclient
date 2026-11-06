//! Platform-independent media metadata, transport state, and change detection.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::Image;

use super::{track_id, MediaUpdate};
use crate::playback_timeline::PlaybackTimeline;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum PlaybackStatus {
    #[default]
    Stopped,
    Paused,
    Playing,
}

impl PlaybackStatus {
    pub(super) fn as_mpris(self) -> &'static str {
        match self {
            Self::Stopped => "Stopped",
            Self::Paused => "Paused",
            Self::Playing => "Playing",
        }
    }
}

pub(super) struct Track {
    server: String,
    song_id: String,
    occurrence: Option<crate::models::PlaybackId>,
    pub(super) path: String,
    pub(super) title: String,
    pub(super) artist: String,
    pub(super) album: String,
    pub(super) duration: Duration,
    pub(super) cover: Option<Arc<Image>>,
}

impl Track {
    fn project(previous: Option<&Arc<Self>>, update: &MediaUpdate<'_>) -> Option<Arc<Self>> {
        let song = update.song?;
        let duration = PlaybackTimeline::for_song(update.playback, song)
            .duration
            .unwrap_or_default();
        let cover_id = update.cover.map(|cover| cover.id);
        if let Some(track) = previous.filter(|track| {
            track.server == update.server
                && track.song_id == song.id
                && track.occurrence == update.occurrence
                && track.title == song.title
                && track.artist == song.artist
                && track.album == song.album
                && track.duration == duration
                && track.cover.as_ref().map(|cover| cover.id) == cover_id
        }) {
            return Some(track.clone());
        }
        Some(Arc::new(Self {
            server: update.server.into(),
            song_id: song.id.clone(),
            occurrence: update.occurrence,
            path: track_id(update.server, &song.id, update.occurrence),
            title: song.title.clone(),
            artist: song.artist.clone(),
            album: song.album.clone(),
            duration,
            cover: update.cover.cloned(),
        }))
    }
}

#[derive(Clone, Default)]
pub(super) struct Snapshot {
    pub(super) track: Option<Arc<Track>>,
    pub(super) status: PlaybackStatus,
    pub(super) position: Duration,
    pub(super) seek_serial: u64,
    pub(super) volume: f64,
    pub(super) can_play: bool,
    pub(super) can_pause: bool,
    pub(super) can_seek: bool,
    pub(super) can_next: bool,
    pub(super) can_previous: bool,
}

impl Snapshot {
    pub(super) fn project(
        previous_track: Option<&Arc<Track>>,
        update: &MediaUpdate<'_>,
        restored_seek_serial: u64,
    ) -> Self {
        let track = Track::project(previous_track, update);
        let playback = update.playback;
        let stopped = track.is_none() || playback.ended || playback.error.is_some();
        let status = if stopped {
            PlaybackStatus::Stopped
        } else if playback.active && !playback.paused {
            PlaybackStatus::Playing
        } else {
            PlaybackStatus::Paused
        };
        Self {
            track,
            status,
            position: update.position,
            seek_serial: playback.seek_serial.wrapping_add(restored_seek_serial),
            volume: update.volume as f64,
            // Queue playback is possible even after Stop cleared now_playing.
            can_play: update.can_play,
            can_pause: !stopped && playback.active,
            can_seek: update.song.is_some() && update.can_play,
            can_next: update.can_next,
            can_previous: update.can_previous,
        }
    }

    /// Position is read on demand. Only a completed or restored seek signals it.
    pub(super) fn changes_from(&self, before: &Self) -> Changes {
        Changes {
            metadata: !same_track(&before.track, &self.track),
            status: before.status != self.status,
            volume: before.volume != self.volume,
            can_play: before.can_play != self.can_play,
            can_pause: before.can_pause != self.can_pause,
            can_seek: before.can_seek != self.can_seek,
            can_next: before.can_next != self.can_next,
            can_previous: before.can_previous != self.can_previous,
            seeked: before.seek_serial != self.seek_serial,
        }
    }
}

pub(super) fn same_track(a: &Option<Arc<Track>>, b: &Option<Arc<Track>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

pub(super) fn micros(value: Duration) -> i64 {
    value.as_micros().min(i64::MAX as u128) as i64
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Changes {
    pub(super) metadata: bool,
    pub(super) status: bool,
    pub(super) volume: bool,
    pub(super) can_play: bool,
    pub(super) can_pause: bool,
    pub(super) can_seek: bool,
    pub(super) can_next: bool,
    pub(super) can_previous: bool,
    pub(super) seeked: bool,
}

impl Changes {
    pub(super) fn any(&self) -> bool {
        self.metadata
            || self.status
            || self.volume
            || self.can_play
            || self.can_pause
            || self.can_seek
            || self.can_next
            || self.can_previous
            || self.seeked
    }
}

#[cfg(test)]
mod tests;
