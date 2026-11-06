//! Shared duration selection and bounded relative seeking for player controls.

use std::time::Duration;

use crate::{audio::PlaybackState, models::Song};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PlaybackTimeline {
    pub(crate) position: Duration,
    pub(crate) duration: Option<Duration>,
}

impl PlaybackTimeline {
    pub(crate) fn for_song(playback: &PlaybackState, song: &Song) -> Self {
        let timeline = Self {
            position: Duration::ZERO,
            // An inactive worker can retain a different track's duration.
            // Only active audio can supersede this song's declared length.
            duration: playback
                .active
                .then_some(playback.duration)
                .flatten()
                .or_else(|| song_duration_hint(song)),
        };
        timeline.with_position(playback.position)
    }

    /// Use one bound for saved positions, displayed progress and seek targets.
    pub(crate) fn with_position(mut self, position: Duration) -> Self {
        self.position = self.duration.map_or(position, |limit| position.min(limit));
        self
    }

    pub(crate) fn seek_seconds(self, offset: i64) -> Duration {
        self.shift(Duration::from_secs(offset.unsigned_abs()), offset < 0)
    }

    pub(crate) fn seek_micros(self, offset: i64) -> Duration {
        self.shift(Duration::from_micros(offset.unsigned_abs()), offset < 0)
    }

    fn shift(self, distance: Duration, backwards: bool) -> Duration {
        let target = if backwards {
            self.position.saturating_sub(distance)
        } else {
            self.position.saturating_add(distance)
        };
        self.with_position(target).position
    }
}

pub(crate) fn song_duration_hint(song: &Song) -> Option<Duration> {
    song.duration
        .and_then(|seconds| u64::try_from(seconds).ok())
        .map(Duration::from_secs)
}

#[cfg(test)]
mod tests;
