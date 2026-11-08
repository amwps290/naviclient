//! Audio commands and application effects for next-track preparation and handoff.

use super::logic::should_preload_next;
use super::occurrence::Selection;
use super::preload_state::PreloadTransitionError;
use super::start::PlaybackRequest;
use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn cancel_next_preload(&mut self) {
        self.preload_resolution.cancel();
        if let Some(event) = self.audio.cancel_preload() {
            self.accept_gapless_transition(event);
        }
        self.preload.clear();
    }

    pub(in crate::app) fn freeze_next_preload(&mut self) {
        self.preload_resolution.cancel();
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
        self.state.now_playing_audio_quality = next.audio_quality;
        self.report_now_playing(&song, Duration::ZERO);
    }

    pub(in crate::app) fn maybe_preload_next(&mut self) {
        if self.preload.reserved()
            || self.preload_resolution.pending()
            || self.start_resolution.pending()
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
        let Some(api) = self.playback_source.as_ref() else {
            return;
        };
        let media = crate::source::MediaKey::new(api.id().clone(), &song.id);
        if self
            .preload_resolution
            .attempted(&media, self.state.occurrence.id())
        {
            return;
        }
        let resource = api.try_resolve_immediate(song, self.config.transcoding_quality);
        let Some(result) = resource else {
            self.resolve_next_preload(index);
            return;
        };
        let Ok(request) = result.and_then(|resource| {
            PlaybackRequest::from_resource(resource, song, self.config.transcoding_quality)
        }) else {
            return;
        };
        let gain = self.song_gain(song);
        let audio = self.audio.clone();
        let cache_key = request.cache_key.clone();
        if self
            .preload
            .reserve(&self.state.queue, index, request.quality, |id| {
                audio.preload(id, request.url, request.cache_key, request.duration, gain)
            })
        {
            self.preload.set_resource(request.audio_quality, cache_key);
        }
    }
}
