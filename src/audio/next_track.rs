//! Next-track reservation, speculative preparation, and output handoff ownership.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use reqwest::blocking::Client;
use rodio::{ChannelCount, SampleRate};

use super::decoder::PreparedStream;
use super::eq::EqHandle;
use super::gapless::{ArmedTrack, AutoTrack, Slot};
use super::output_track::OutputTrack;
use super::preload::Preloader;
use super::track_transition::{TrackTransition, Transition};

pub(super) struct PreloadRequest {
    pub(super) id: u64,
    pub(super) url: crate::source::HttpResource,
    pub(super) cache_key: String,
    pub(super) duration: Option<Duration>,
    pub(super) gain: f32,
    pub(super) transition: Arc<Transition>,
}

pub(super) struct NextTrack {
    preloader: Option<Preloader>,
    handoff: Slot,
    pending: Option<PreloadRequest>,
    active: Option<PreloadRequest>,
    armed: Option<AutoTrack>,
}

impl NextTrack {
    pub(super) fn new(client: Client, handoff: Slot) -> Self {
        let preloader = Preloader::new(client)
            .map_err(|error| {
                log::warn!("next-track preloading unavailable: {error:#}");
                error
            })
            .ok();
        Self::with_preloader(preloader, handoff)
    }

    fn with_preloader(preloader: Option<Preloader>, handoff: Slot) -> Self {
        Self {
            preloader,
            handoff,
            pending: None,
            active: None,
            armed: None,
        }
    }

    /// Replace only a deferred request. The frontend cancels an existing
    /// reservation before replacing an active or armed next track.
    pub(super) fn reserve(&mut self, request: PreloadRequest) {
        self.pending = Some(request);
    }

    pub(super) fn take_started(&mut self) -> Option<(AutoTrack, TrackTransition)> {
        let event = self.armed.as_ref()?.transition.event()?;
        let next = self.armed.take()?;
        self.active = None;
        Some((next, event))
    }

    /// Detach an already normalized track before stopping the old output. A
    /// cancelled reservation can still be reused for a matching manual Play.
    pub(super) fn begin_play(
        &mut self,
        id: Option<u64>,
        cache_key: &str,
        requested_position: Duration,
    ) -> Option<OutputTrack> {
        self.pending = None;
        self.active = None;
        self.armed = None;
        self.handoff
            .lock()
            .unwrap()
            .take()
            .filter(|next| {
                id == Some(next.transition.id)
                    && next.cache_key == cache_key
                    && requested_position.is_zero()
            })
            .map(|next| next.track)
    }

    /// Called after the current Player has been cleared, so promotion does not
    /// start decoding the next track while the old output is being stopped.
    pub(super) fn take_prepared(
        &mut self,
        id: Option<u64>,
        cache_key: &str,
        position: Duration,
    ) -> Option<PreparedStream> {
        self.preloader.as_mut()?.take(id, cache_key, position)
    }

    pub(super) fn cancel(&mut self) {
        self.handoff.lock().unwrap().take();
        self.active = None;
        self.armed = None;
        self.pending = None;
        if let Some(preloader) = self.preloader.as_mut() {
            preloader.cancel();
        }
    }

    pub(super) fn poll(
        &mut self,
        can_preload: bool,
        cache_dir: &Path,
        channels: ChannelCount,
        sample_rate: SampleRate,
        eq: &EqHandle,
    ) {
        if let Some(preloader) = self.preloader.as_mut() {
            preloader.poll();
        }
        self.start_pending(can_preload, cache_dir);
        self.arm_ready(channels, sample_rate, eq);
    }

    fn start_pending(&mut self, can_preload: bool, cache_dir: &Path) {
        if !can_preload {
            return;
        }
        let Some(request) = self.pending.take() else {
            return;
        };
        if let Some(preloader) = self
            .preloader
            .as_mut()
            .filter(|_| request.transition.pending())
        {
            preloader.request(
                request.id,
                request.url.clone(),
                request.cache_key.clone(),
                request.duration,
                cache_dir.to_path_buf(),
            );
            self.active = Some(request);
        }
    }

    fn arm_ready(&mut self, channels: ChannelCount, sample_rate: SampleRate, eq: &EqHandle) {
        if self.armed.is_some() {
            return;
        }
        let Some(request) = self
            .active
            .as_ref()
            .filter(|request| request.transition.pending())
        else {
            return;
        };
        let Some(stream) = self.preloader.as_mut().and_then(Preloader::take_ready) else {
            return;
        };
        let track = OutputTrack::new(stream, request.gain, channels, sample_rate, eq.clone());
        self.armed = Some(AutoTrack {
            transition: request.transition.clone(),
            url: request.url.clone(),
            cache_key: request.cache_key.clone(),
            gain: request.gain,
            info: track.info.clone(),
        });
        *self.handoff.lock().unwrap() = Some(ArmedTrack {
            transition: request.transition.clone(),
            cache_key: request.cache_key.clone(),
            track,
        });
    }
}

impl Drop for NextTrack {
    fn drop(&mut self) {
        // Clear the published handoff as well as the preparation mailbox, so
        // an output callback cannot start orphaned audio during worker shutdown.
        self.cancel();
    }
}

#[cfg(test)]
mod tests;
