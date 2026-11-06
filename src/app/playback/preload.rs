//! Audio commands and application effects for next-track preparation and handoff.

use super::logic::should_preload_next;
use super::occurrence::Selection;
use super::preload_state::PreloadTransitionError;
use super::start::PlaybackRequest;
use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn cancel_next_preload(&mut self) {
        if let Some(event) = self.audio.cancel_preload() {
            self.accept_gapless_transition(event);
        }
        self.preload.clear();
    }

    pub(in crate::app) fn freeze_next_preload(&mut self) {
        if let Some(event) = self.audio.freeze_preload() {
            self.accept_gapless_transition(event);
        }
    }

    pub(in crate::app) fn sync_gapless_transition(&mut self) {
        if let Some(event) = self.audio.take_transition() {
            self.accept_gapless_transition(event);
        }
    }

    pub(in crate::app) fn accept_gapless_transition(&mut self, event: TrackTransition) {
        let next = match self.preload.take_transition(event.id, &self.state.queue) {
            Ok(next) => next,
            Err(PreloadTransitionError::Unreserved) => {
                log::error!("missing UI reservation for gapless transition {}", event.id);
                return;
            }
            Err(PreloadTransitionError::QueueChanged) => return,
        };
        let song = self.state.queue.song(next.index).cloned().unwrap();
        self.ensure_scrobble_on_end();
        self.finish_listening_event(true);
        self.cache_measured_loudness(event.loudness_db, event.loudness_peak);
        if self.state.playback_modes.shuffle
            && self.state.shuffle_history.restore_forward().is_none()
        {
            self.state.shuffle_history.advance(next.index);
        }
        self.select_playing(next.index, &song, next.quality, Selection::New);
        self.report_now_playing(&song, Duration::ZERO);
    }

    pub(in crate::app) fn maybe_preload_next(&mut self) {
        if self.preload.reserved()
            || !self.state.occurrence.active()
            || !should_preload_next(&self.audio.state())
        {
            return;
        }
        let Some(index) = self.next_queue_index(true) else {
            return;
        };
        let Some(song) = self.state.queue.song(index) else {
            return;
        };
        let Some(api) = self.api.as_ref() else { return };
        let Ok(request) = PlaybackRequest::prepare(api, song, self.config.transcoding_quality)
        else {
            return;
        };
        let gain = self.song_gain(song);
        let audio = self.audio.clone();
        self.preload
            .reserve(&self.state.queue, index, request.quality, |id| {
                audio.preload(id, request.url, request.cache_key, request.duration, gain)
            });
    }
}
