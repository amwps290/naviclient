//! Optional, read-only waveform envelopes from the samples consumed by the player.
//!
//! The output callback never allocates or locks. A bounded atomic snapshot is
//! enough for a display; analysis is bypassed while the fullscreen view is hidden.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rodio::{ChannelCount, SampleRate, Source};

pub(crate) const WAVEFORM_BINS: usize = 64;
const FRAMES_PER_SECOND: usize = 30;

pub(crate) struct AudioVisualizer {
    enabled: AtomicBool,
    envelope: [AtomicU32; WAVEFORM_BINS],
}

impl Default for AudioVisualizer {
    fn default() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            envelope: std::array::from_fn(|_| AtomicU32::new(0.0_f32.to_bits())),
        }
    }
}

impl AudioVisualizer {
    pub(crate) fn set_enabled(&self, enabled: bool) {
        if self.enabled.swap(enabled, Ordering::AcqRel) != enabled {
            self.clear();
        }
    }

    pub(crate) fn snapshot(&self) -> [f32; WAVEFORM_BINS] {
        if !self.enabled.load(Ordering::Acquire) {
            return [0.0; WAVEFORM_BINS];
        }
        std::array::from_fn(|index| f32::from_bits(self.envelope[index].load(Ordering::Relaxed)))
    }

    fn clear(&self) {
        for bin in &self.envelope {
            bin.store(0.0_f32.to_bits(), Ordering::Relaxed);
        }
    }

    fn publish(&self, frame: &[f32; WAVEFORM_BINS]) {
        for (bin, value) in self.envelope.iter().zip(frame) {
            bin.store(value.to_bits(), Ordering::Relaxed);
        }
    }
}

pub(super) struct WaveformSource<S> {
    inner: S,
    shared: Arc<AudioVisualizer>,
    frame: [f32; WAVEFORM_BINS],
    index: usize,
    samples: usize,
    samples_per_bin: usize,
    peak: f32,
    was_enabled: bool,
}

impl<S> WaveformSource<S> {
    pub(super) fn new(inner: S, shared: Arc<AudioVisualizer>) -> Self {
        Self {
            inner,
            shared,
            frame: [0.0; WAVEFORM_BINS],
            index: 0,
            samples: 0,
            samples_per_bin: 1,
            peak: 0.0,
            was_enabled: false,
        }
    }

    fn reset(&mut self) {
        self.index = 0;
        self.samples = 0;
        self.peak = 0.0;
    }
}

impl<S: Source<Item = f32>> Iterator for WaveformSource<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sample = self.inner.next()?;
        if !self.shared.enabled.load(Ordering::Relaxed) {
            if self.was_enabled {
                self.reset();
                self.was_enabled = false;
            }
            return Some(sample);
        }
        self.was_enabled = true;
        if self.index == 0 && self.samples == 0 {
            // A player's queued source can change format as a track starts.
            self.samples_per_bin = (self.inner.sample_rate().get() as usize
                * usize::from(self.inner.channels().get())
                / (FRAMES_PER_SECOND * WAVEFORM_BINS))
                .max(1);
        }
        if sample.is_finite() {
            // Peaks across channels preserve opposite-phase stereo material.
            self.peak = self.peak.max(sample.abs().min(1.0));
        }
        self.samples += 1;
        if self.samples >= self.samples_per_bin {
            self.frame[self.index] = self.peak;
            self.index += 1;
            self.samples = 0;
            self.peak = 0.0;
            if self.index == WAVEFORM_BINS {
                self.shared.publish(&self.frame);
                self.index = 0;
            }
        }
        Some(sample)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S: Source<Item = f32>> Source for WaveformSource<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, position: Duration) -> Result<(), rodio::source::SeekError> {
        self.inner.try_seek(position)?;
        self.reset();
        self.shared.clear();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
