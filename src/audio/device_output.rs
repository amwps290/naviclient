//! Output-device ownership and replacement, independent of stream preparation.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use rodio::{ChannelCount, Player, SampleRate};

use super::eq::{new_eq_handle, EqHandle};
use super::gapless;
use super::output::open_audio_device;
use super::{set_state, PlaybackState};
use crate::models::EqSettings;

pub(super) struct DevicePlayback {
    /// The stream stays alive until this entire device playback is replaced.
    /// Hardware-free tests keep only the player and its source.
    _sink: Option<rodio::MixerDeviceSink>,
    pub(super) name: Option<String>,
    pub(super) player: Player,
    pub(super) channels: ChannelCount,
    pub(super) sample_rate: SampleRate,
}

impl DevicePlayback {
    pub(super) fn new(sink: rodio::MixerDeviceSink, name: Option<String>) -> Self {
        let channels = sink.config().channel_count();
        let sample_rate = sink.config().sample_rate();
        let (player, source) = Player::new();
        sink.mixer().add(source);
        Self {
            _sink: Some(sink),
            name,
            player,
            channels,
            sample_rate,
        }
    }

    fn open_for_switch(target: Option<&str>) -> Result<Self> {
        let (mut sink, name) = open_audio_device(target)?;
        sink.log_on_drop(false);
        Ok(Self::new(sink, name))
    }

    fn report(&self, state: &Arc<Mutex<PlaybackState>>) {
        set_state(state, |s| {
            s.output_name = self.name.clone();
            s.output_sample_rate = self.sample_rate.get();
            s.output_channels = self.channels.get();
        });
    }

    #[cfg(test)]
    pub(super) fn headless(
        player: Player,
        name: Option<String>,
        channels: ChannelCount,
        sample_rate: SampleRate,
    ) -> Self {
        Self {
            _sink: None,
            name,
            player,
            channels,
            sample_rate,
        }
    }
}

pub(super) struct AudioOutput {
    pub(super) device: DevicePlayback,
    /// These handles survive output changes: preparation and gapless decoding
    /// still refer to them until the worker's existing Seek path cancels a handoff.
    pub(super) handoff: gapless::Slot,
    pub(super) eq: EqHandle,
}

impl AudioOutput {
    pub(super) fn new(device: DevicePlayback) -> Self {
        Self {
            device,
            handoff: Arc::new(Mutex::new(None)),
            eq: new_eq_handle(EqSettings::default()),
        }
    }

    pub(super) fn switch_device(
        &mut self,
        target: Option<&str>,
        state: &Arc<Mutex<PlaybackState>>,
    ) -> Result<()> {
        self.switch_with(state, || DevicePlayback::open_for_switch(target))
    }

    fn switch_with(
        &mut self,
        state: &Arc<Mutex<PlaybackState>>,
        open: impl FnOnce() -> Result<DevicePlayback>,
    ) -> Result<()> {
        // Opening can fail or block. Keep every part of the old output alive
        // until a complete replacement (player, stream and format) is ready.
        let device = open()?;
        log::info!(
            "audio output switched; name={:?} channels={} sample_rate={}",
            device.name,
            device.channels.get(),
            device.sample_rate.get()
        );
        self.device = device;
        self.device.report(state);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
