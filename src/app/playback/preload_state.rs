//! Frontend ownership of accepted next-track reservations and gapless events.

use std::time::Duration;

use crate::models::{PlaybackQueue, QueueEntryId, TranscodingQuality};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) struct NextPreload {
    pub(in crate::app) id: u64,
    pub(in crate::app) index: usize,
    pub(in crate::app) entry: QueueEntryId,
    pub(in crate::app) quality: TranscodingQuality,
}

#[derive(Debug, PartialEq, Eq)]
pub(in crate::app) enum PreloadTransitionError {
    Unreserved,
    QueueChanged,
}

/// This state survives server resets so a new reservation cannot reuse an old
/// transition's ID. Cancelling clears the reservation, but retains the sequence.
#[derive(Default)]
pub(in crate::app) struct PreloadState {
    serial: u64,
    pending: Option<NextPreload>,
}

impl PreloadState {
    pub(in crate::app) fn reserved(&self) -> bool {
        self.pending.is_some()
    }

    /// Publish a reservation only after the audio command has been accepted.
    /// Rejected commands consume an ID but cannot block a subsequent attempt.
    pub(in crate::app) fn reserve(
        &mut self,
        queue: &PlaybackQueue,
        index: usize,
        quality: TranscodingQuality,
        submit: impl FnOnce(u64) -> bool,
    ) -> bool {
        if self.reserved() {
            return false;
        }
        let Some(entry) = queue.entry_id(index) else {
            return false;
        };
        let Some(id) = self.serial.checked_add(1) else {
            return false;
        };
        self.serial = id;
        if !submit(id) {
            return false;
        }
        self.pending = Some(NextPreload {
            id,
            index,
            entry,
            quality,
        });
        true
    }

    pub(in crate::app) fn reserved_index(&self, queue: &PlaybackQueue) -> Option<usize> {
        let next = self.pending?;
        (queue.entry_id(next.index) == Some(next.entry)).then_some(next.index)
    }

    pub(in crate::app) fn reusable_id(
        &self,
        entry: QueueEntryId,
        quality: TranscodingQuality,
        position: Duration,
    ) -> Option<u64> {
        self.pending
            .filter(|next| next.entry == entry && next.quality == quality && position.is_zero())
            .map(|next| next.id)
    }

    /// An unrelated event leaves the current reservation intact. A matching
    /// event retires it once, even if its queue occurrence no longer exists.
    pub(in crate::app) fn take_transition(
        &mut self,
        id: u64,
        queue: &PlaybackQueue,
    ) -> Result<NextPreload, PreloadTransitionError> {
        let next = self
            .pending
            .filter(|next| next.id == id)
            .ok_or(PreloadTransitionError::Unreserved)?;
        self.pending = None;
        if queue.entry_id(next.index) != Some(next.entry) {
            return Err(PreloadTransitionError::QueueChanged);
        }
        Ok(next)
    }

    pub(in crate::app) fn clear(&mut self) {
        self.pending = None;
    }
}

#[cfg(test)]
mod tests;
