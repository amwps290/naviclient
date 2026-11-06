//! Continuous sample output and handoff to the prepared track at its boundary.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rodio::{ChannelCount, Sample, SampleRate, Source};

use super::decoder::decoded_duration;
use super::output_track::{OutputTrack, TrackInfo};
use super::track_transition::Transition;

pub(super) type Slot = Arc<Mutex<Option<ArmedTrack>>>;

pub(super) struct ArmedTrack {
    pub(super) transition: Arc<Transition>,
    pub(super) cache_key: String,
    pub(super) track: OutputTrack,
}

pub(super) struct AutoTrack {
    pub(super) transition: Arc<Transition>,
    pub(super) url: String,
    pub(super) cache_key: String,
    pub(super) gain: f32,
    pub(super) info: TrackInfo,
}

/// Uniform output format throughout the chain. next() crosses the track boundary
/// in the same call, without inserting silence or waiting for the UI/worker.
pub(super) struct GaplessSource {
    current: OutputTrack,
    slot: Slot,
    channels: ChannelCount,
    rate: SampleRate,
    samples: u64,
    finished: bool,
}

impl GaplessSource {
    pub(super) fn new(
        current: OutputTrack,
        slot: Slot,
        channels: ChannelCount,
        rate: SampleRate,
    ) -> Self {
        Self {
            current,
            slot,
            channels,
            rate,
            samples: 0,
            finished: false,
        }
    }
}

impl Iterator for GaplessSource {
    type Item = Sample;
    fn next(&mut self) -> Option<Sample> {
        if self.finished {
            return None;
        }
        if let Some(sample) = self.current.next_sample() {
            self.samples += 1;
            return Some(sample);
        }
        let next = {
            let mut slot = self.slot.lock().unwrap();
            let healthy = self.current.info.error.lock().unwrap().is_none();
            if healthy
                && slot.as_ref().is_some_and(|next| {
                    next.transition.start(
                        decoded_duration(self.samples, self.channels, self.rate),
                        &self.current.info.loudness,
                    )
                })
            {
                slot.take()
            } else {
                None
            }
        };
        if let Some(next) = next {
            next.track.info.control.promote();
            self.current = next.track;
            // A prepared track contains startup samples, so this does not wait.
            self.next()
        } else {
            self.finished = true;
            None
        }
    }
}

impl Source for GaplessSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        self.channels
    }
    fn sample_rate(&self) -> SampleRate {
        self.rate
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
pub(super) mod tests;
