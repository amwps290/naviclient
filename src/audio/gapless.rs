use super::*;

type OutputSource = Box<dyn Source<Item = Sample> + Send>;
pub(super) type Loudness = Arc<Mutex<Option<(f32, f32)>>>;
pub(super) type Slot = Arc<Mutex<Option<ArmedTrack>>>;

#[derive(Clone, Copy, Debug)]
pub struct TrackTransition {
    pub id: u64,
    pub loudness_db: f32,
    pub loudness_peak: f32,
    pub(super) boundary: Duration,
}

#[derive(Debug, Default)]
enum Phase {
    #[default]
    Pending,
    Cancelled,
    Started(TrackTransition),
}

/// Created when the UI reserves a song, before either worker sees the request.
/// Cancellation and the first sample share this lock, so exactly one wins.
#[derive(Debug)]
pub(super) struct Transition {
    pub(super) id: u64,
    pub(super) duration: Option<Duration>,
    phase: Mutex<Phase>,
    reported: std::sync::atomic::AtomicBool,
}

impl Transition {
    pub(super) fn new(id: u64, duration: Option<Duration>) -> Arc<Self> {
        Arc::new(Self {
            id,
            duration,
            phase: Mutex::new(Phase::Pending),
            reported: false.into(),
        })
    }

    pub(super) fn cancel(&self) {
        let mut phase = self.phase.lock().unwrap();
        if matches!(*phase, Phase::Pending) {
            *phase = Phase::Cancelled;
        }
    }

    pub(super) fn pending(&self) -> bool {
        matches!(*self.phase.lock().unwrap(), Phase::Pending)
    }

    pub(super) fn event(&self) -> Option<TrackTransition> {
        match *self.phase.lock().unwrap() {
            Phase::Started(event) => Some(event),
            _ => None,
        }
    }

    pub(super) fn take_event(&self) -> Option<TrackTransition> {
        self.event()
            .filter(|_| !self.reported.swap(true, Ordering::AcqRel))
    }

    fn start(&self, boundary: Duration, previous: &Loudness) -> bool {
        let mut phase = self.phase.lock().unwrap();
        if !matches!(*phase, Phase::Pending) {
            return false;
        }
        let (loudness_db, loudness_peak) =
            previous.lock().ok().and_then(|v| *v).unwrap_or_default();
        *phase = Phase::Started(TrackTransition {
            id: self.id,
            boundary,
            loudness_db,
            loudness_peak,
        });
        true
    }
}

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
    ) -> Self {
        let info = TrackInfo {
            duration: stream.duration,
            error: stream.error,
            buffer: stream.buffer_progress,
            control: stream.source.control.clone(),
            loudness: Arc::new(Mutex::new(None)),
        };
        let metered = LoudnessMeter::new(stream.source, info.loudness.clone());
        Self {
            source: Box::new(normalize_for_output(
                metered.amplify(gain.clamp(0.1, 4.0)),
                channels,
                rate,
            )),
            info,
        }
    }
}

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
        if let Some(sample) = self.current.source.next() {
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
mod tests;
