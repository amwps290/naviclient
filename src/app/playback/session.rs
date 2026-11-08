//! Session persistence and restoration without starting audio playback.

use crate::app::prelude::*;
use crate::playback_timeline::PlaybackTimeline;

impl NavidromeApp {
    pub(in crate::app) fn save_session(&mut self) {
        let Some(snapshot) = self.session.snapshot(&self.state, &self.audio.state()) else {
            return;
        };
        let submitted = match self.session.source() {
            Some(source) => self.server_cache_writer.submit_source(source, snapshot),
            None => self
                .server_cache_writer
                .submit(self.session.server_id(), snapshot),
        };
        match submitted {
            Ok(()) => self.session.submitted(std::time::Instant::now()),
            Err(error) => log::warn!("could not queue playback session: {error}"),
        }
    }

    pub(in crate::app) fn flush_server_cache_writes(&self) {
        if let Err(error) = self.server_cache_writer.flush() {
            log::warn!("pending server cache save failed: {error}");
        }
    }

    pub(in crate::app) fn save_playback_before_exit(&mut self) {
        self.start_resolution.cancel();
        self.cancel_next_preload();
        self.finish_listening_event(false);
        self.save_session();
        self.flush_server_cache_writes();
    }

    pub(in crate::app) fn maybe_save_session(&mut self) {
        if self
            .session
            .should_save(std::time::Instant::now(), !self.state.queue.is_empty())
        {
            self.save_session();
        }
    }

    pub(in crate::app) fn restore_session(&mut self, session: PlaybackSession) {
        if !self.session.restore(&mut self.state, session) {
            return;
        }
        self.config.playback_modes = self.state.playback_modes;
        if let Some(song) = self.state.now_playing.clone() {
            self.ensure_song_cover(&song, true);
        }
        log::info!(
            "restored playback session; server={:?} queue={} index={:?} position={:.1}s",
            self.session.server_id(),
            self.state.queue.len(),
            self.state.queue_index,
            self.playback_timeline()
                .map_or(Duration::ZERO, |timeline| timeline.position)
                .as_secs_f64(),
        );
    }

    pub(in crate::app) fn playback_for_display(&self) -> PlaybackState {
        self.session
            .display_playback(self.audio.state(), self.state.now_playing.as_ref())
    }

    pub(in crate::app) fn playback_timeline(&self) -> Option<PlaybackTimeline> {
        self.session
            .timeline(&self.audio.state(), self.state.now_playing.as_ref())
    }
}
