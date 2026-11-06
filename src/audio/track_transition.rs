//! Track-boundary start/cancel arbitration and once-only event consumption.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::loudness_meter::Loudness;

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

    pub(super) fn start(&self, boundary: Duration, previous: &Loudness) -> bool {
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

#[cfg(test)]
mod tests;
