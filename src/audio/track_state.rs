//! Current-track source, diagnostics, logical clock, and playback-state projection.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::error::FailureError;
use super::gapless::AutoTrack;
use super::http_stream::logical_playback_position;
use super::loudness_meter::Loudness;
use super::output_track::TrackInfo;
use super::preparation::StreamRequest;
use super::preparation_state::PreparationState;
use super::track_transition::TrackTransition;
use super::PlaybackState;

const STALL_VISIBLE_AFTER: Duration = Duration::from_secs(3);

struct TrackSource {
    url: crate::source::HttpResource,
    cache_key: String,
    gain: f32,
}

pub(super) struct TrackState {
    source: Option<TrackSource>,
    duration: Option<Duration>,
    info: Option<TrackInfo>,
    loudness: Option<Loudness>,
    startup_error: Option<FailureError>,
    position_offset: Duration,
    boundary: Duration,
    transition_id: Option<u64>,
    terminal_logged: bool,
}

impl Default for TrackState {
    fn default() -> Self {
        Self {
            source: None,
            duration: None,
            info: None,
            loudness: None,
            startup_error: None,
            position_offset: Duration::ZERO,
            boundary: Duration::ZERO,
            transition_id: None,
            terminal_logged: true,
        }
    }
}

impl TrackState {
    pub(super) fn begin_play(
        &mut self,
        url: crate::source::HttpResource,
        cache_key: String,
        duration: Option<Duration>,
        gain: f32,
        position: Duration,
    ) -> Duration {
        let position = duration.map_or(position, |limit| position.min(limit));
        *self = Self {
            source: Some(TrackSource {
                url,
                cache_key,
                gain: gain.clamp(0.1, 4.0),
            }),
            duration,
            position_offset: position,
            terminal_logged: false,
            ..Self::default()
        };
        position
    }

    pub(super) fn prepare_seek(
        &mut self,
        position: Duration,
        cache_dir: PathBuf,
    ) -> Option<(Duration, StreamRequest)> {
        let source = self.source.as_ref()?;
        let request = StreamRequest {
            url: source.url.clone(),
            cache_key: source.cache_key.clone(),
            cache_dir,
            duration: self.duration,
        };
        let position = self.duration.map_or(position, |limit| position.min(limit));
        // A seek keeps this occurrence and its measured loudness, but replaces
        // decoding/buffering diagnostics and restarts the output clock.
        self.info = None;
        self.startup_error = None;
        self.position_offset = position;
        self.boundary = Duration::ZERO;
        self.terminal_logged = false;
        Some((position, request))
    }

    pub(super) fn accept_ready(&mut self, info: TrackInfo) {
        self.duration = info.duration;
        self.loudness = Some(info.loudness.clone());
        self.info = Some(info);
    }

    pub(super) fn accept_gapless(&mut self, next: AutoTrack, event: TrackTransition) {
        *self = Self {
            source: Some(TrackSource {
                url: next.url,
                cache_key: next.cache_key,
                gain: next.gain,
            }),
            boundary: event.boundary,
            transition_id: Some(event.id),
            terminal_logged: false,
            ..Self::default()
        };
        self.accept_ready(next.info);
    }

    pub(super) fn stop(&mut self) {
        *self = Self::default();
    }

    pub(super) fn record_error(&mut self, error: FailureError) {
        self.startup_error = Some(error);
    }

    pub(super) fn gain(&self) -> f32 {
        self.source.as_ref().map_or(1.0, |source| source.gain)
    }

    pub(super) fn duration(&self) -> Option<Duration> {
        self.duration
    }

    pub(super) fn position(&self, output_position: Duration) -> Duration {
        if self.source.is_none() {
            return Duration::ZERO;
        }
        let silence = self
            .info
            .as_ref()
            .map(|info| info.control.synthetic_silence_duration())
            .unwrap_or_default();
        logical_playback_position(
            self.position_offset,
            output_position.saturating_sub(self.boundary),
            silence,
        )
    }

    pub(super) fn fully_buffered(&self) -> bool {
        self.info
            .as_ref()
            .is_some_and(|info| info.control.fully_decoded())
    }

    pub(super) fn take_terminal_log(&mut self, empty: bool, preparing: bool) -> bool {
        if !empty || preparing || self.terminal_logged {
            return false;
        }
        self.terminal_logged = true;
        true
    }

    pub(super) fn snapshot(
        &self,
        output: OutputSnapshot,
        preparation: &PreparationState,
        now: Instant,
    ) -> TrackSnapshot {
        let preparing = preparation.preparing();
        let paused = if preparing {
            preparation.paused()
        } else {
            output.paused
        };
        let error = if output.empty && !preparing {
            self.startup_error
                .clone()
                .or_else(|| self.info.as_ref()?.error.lock().ok()?.clone())
        } else {
            None
        };
        let failure = error.as_ref().map(|error| error.kind);
        let error = error.map(|error| error.message());
        let position = self.position(output.position);
        let buffered = self
            .duration
            .zip(self.info.as_ref())
            .map(|(duration, info)| info.buffer.buffered_duration(duration))
            .unwrap_or_default();
        let stall = stall_duration(
            preparation.elapsed(now),
            output.empty,
            paused,
            self.info.as_ref().map(|info| info.control.idle()),
        );
        let (loudness_db, loudness_peak) = self
            .loudness
            .as_ref()
            .and_then(|loudness| *loudness.lock().ok()?)
            .unwrap_or((0.0, 0.0));
        TrackSnapshot {
            playback: PlaybackState {
                position: self.duration.map_or(position, |limit| position.min(limit)),
                duration: self.duration,
                buffered,
                fully_buffered: self.fully_buffered(),
                transition_id: self.transition_id,
                paused,
                active: preparing || !output.empty,
                output_connected: !preparing && self.info.is_some() && !output.empty,
                // Launch and Stop leave an empty output without ending a track.
                ended: self.source.is_some() && output.empty && !preparing && error.is_none(),
                error,
                failure,
                stall,
                loudness_db,
                loudness_peak,
                ..Default::default()
            },
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct OutputSnapshot {
    pub(super) position: Duration,
    pub(super) paused: bool,
    pub(super) empty: bool,
}

pub(super) struct TrackSnapshot {
    pub(super) playback: PlaybackState,
}

impl TrackSnapshot {
    pub(super) fn apply(self, state: &mut PlaybackState) {
        // Seek completion serial and device diagnostics belong to the worker,
        // and survive track snapshots, resets, and gapless handoffs.
        let snapshot = self.playback;
        state.position = snapshot.position;
        state.duration = snapshot.duration;
        state.buffered = snapshot.buffered;
        state.fully_buffered = snapshot.fully_buffered;
        state.transition_id = snapshot.transition_id;
        state.paused = snapshot.paused;
        state.active = snapshot.active;
        state.output_connected = snapshot.output_connected;
        state.ended = snapshot.ended;
        state.error = snapshot.error;
        state.failure = snapshot.failure;
        state.stall = snapshot.stall;
        state.loudness_db = snapshot.loudness_db;
        state.loudness_peak = snapshot.loudness_peak;
    }
}

fn stall_duration(
    preparing_elapsed: Option<Duration>,
    empty: bool,
    paused: bool,
    decode_idle: Option<Duration>,
) -> Option<Duration> {
    let elapsed = match preparing_elapsed {
        Some(elapsed) => Some(elapsed),
        None if !empty && !paused => decode_idle,
        None => None,
    }?;
    (elapsed >= STALL_VISIBLE_AFTER).then(|| Duration::from_secs(elapsed.as_secs()))
}

#[cfg(test)]
mod tests;
