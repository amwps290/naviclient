//! Prepared-track diagnostics and per-track output processing before handoff.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rodio::{ChannelCount, Sample, SampleRate, Source};

use super::decoder::PreparedStream;
use super::error::SharedStreamError;
use super::loudness_meter::{Loudness, LoudnessMeter};
use super::output::normalize_for_output;
use super::stream_cache::BufferProgress;
use super::stream_control::StreamControl;

type OutputSource = Box<dyn Source<Item = Sample> + Send>;

#[derive(Clone)]
pub(super) struct TrackInfo {
    pub(super) duration: Option<Duration>,
    pub(super) error: SharedStreamError,
    pub(super) buffer: Arc<BufferProgress>,
    pub(super) control: Arc<StreamControl>,
    pub(super) loudness: Loudness,
}

pub(super) struct OutputTrack {
    source: OutputSource,
    pub(super) info: TrackInfo,
}

impl OutputTrack {
    pub(super) fn new(
        stream: PreparedStream,
        gain: f32,
        channels: ChannelCount,
        rate: SampleRate,
        eq: crate::audio::eq::EqHandle,
    ) -> Self {
        let info = TrackInfo {
            duration: stream.duration,
            error: stream.error,
            buffer: stream.buffer_progress,
            control: stream.source.control().clone(),
            loudness: Arc::new(Mutex::new(None)),
        };
        let metered = LoudnessMeter::new(stream.source, info.loudness.clone());
        // 均衡插在 ReplayGain 增益之后、输出归一化之前：
        // 音轨原生采样率下滤波，且不影响响度测量。
        let equalized = crate::audio::eq::Equalizer::new(metered.amplify(gain.clamp(0.1, 4.0)), eq);
        Self {
            source: Box::new(normalize_for_output(equalized, channels, rate)),
            info,
        }
    }

    pub(super) fn next_sample(&mut self) -> Option<Sample> {
        self.source.next()
    }

    #[cfg(test)]
    pub(super) fn into_source(self) -> OutputSource {
        self.source
    }
}

#[cfg(test)]
mod tests;
