//! Frontend ownership of accepted next-track reservations and gapless events.

use std::time::Duration;

use crate::models::{PlaybackQueue, QueueEntryId, TranscodingQuality};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) struct NextPreload {
    pub(in crate::app) id: u64,
    pub(in crate::app) index: usize,
    pub(in crate::app) entry: QueueEntryId,
    pub(in crate::app) quality: TranscodingQuality,
    pub(in crate::app) audio_quality: Option<crate::source::AudioQuality>,
    cache_key: Option<String>,
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
            audio_quality: None,
            cache_key: None,
        });
        true
    }

    pub(in crate::app) fn set_resource(
        &mut self,
        quality: Option<crate::source::AudioQuality>,
        cache_key: String,
    ) {
        if let Some(next) = &mut self.pending {
            next.audio_quality = quality;
            next.cache_key = Some(cache_key);
        }
    }

    pub(in crate::app) fn reserved_index(&self, queue: &PlaybackQueue) -> Option<usize> {
        let next = self.pending.as_ref()?;
        (queue.entry_id(next.index) == Some(next.entry)).then_some(next.index)
    }

    pub(in crate::app) fn reusable_id(
        &self,
        entry: QueueEntryId,
        quality: TranscodingQuality,
        position: Duration,
    ) -> Option<u64> {
        self.pending
            .as_ref()
            .filter(|next| next.entry == entry && next.quality == quality && position.is_zero())
            .map(|next| next.id)
    }

    pub(in crate::app) fn reusable_resource_id(
        &self,
        entry: QueueEntryId,
        quality: TranscodingQuality,
        position: Duration,
        cache_key: &str,
    ) -> Option<u64> {
        self.reusable_id(entry, quality, position).filter(|_| {
            self.pending
                .as_ref()
                .is_some_and(|next| next.cache_key.as_deref().is_none_or(|key| key == cache_key))
        })
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
            .as_ref()
            .filter(|next| next.id == id)
            .cloned()
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
