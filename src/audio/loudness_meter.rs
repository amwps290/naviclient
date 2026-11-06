//! Cumulative raw-sample loudness, independent of devices and output effects.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rodio::{ChannelCount, Sample, SampleRate, Source};

pub(super) type Loudness = Arc<Mutex<Option<(f32, f32)>>>;

#[derive(Default)]
struct SampleLoudness {
    sum_squares: f64,
    sample_count: u64,
    peak: f32,
}

impl SampleLoudness {
    fn observe(&mut self, sample: Sample) -> Option<(f32, f32)> {
        let value = f64::from(sample);
        self.sum_squares += value * value;
        self.sample_count += 1;
        self.peak = self.peak.max(sample.abs());
        // Count interleaved samples, not frames. Keep the cumulative reading
        // and the original 4096-sample cadence without flushing a partial tail.
        if self.sample_count & 4095 != 0 {
            return None;
        }
        let rms = (self.sum_squares / self.sample_count as f64).sqrt();
        let db = if rms > 1e-6 {
            20.0 * rms.log10()
        } else {
            -100.0
        };
        Some((db as f32, self.peak))
    }
}

pub(super) struct LoudnessMeter<S> {
    inner: S,
    samples: SampleLoudness,
    shared: Loudness,
}

impl<S: Source> LoudnessMeter<S> {
    pub(super) fn new(inner: S, shared: Loudness) -> Self {
        Self {
            inner,
            samples: SampleLoudness::default(),
            shared,
        }
    }
}

impl<S: Source> Iterator for LoudnessMeter<S> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        let sample = self.inner.next()?;
        if let Some(reading) = self.samples.observe(sample) {
            if let Ok(mut shared) = self.shared.lock() {
                *shared = Some(reading);
            }
        }
        Some(sample)
    }
}

impl<S: Source> Source for LoudnessMeter<S> {
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
}

#[cfg(test)]
mod tests;
