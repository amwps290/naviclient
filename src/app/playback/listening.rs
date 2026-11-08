//! Local listening observation and server-scoped persistence.

use super::listening_state::ListeningUpdate;
use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn poll_listening_history(&mut self) {
        let song = self.state.now_playing.clone();
        let update = self.state.listening.observe(
            song.as_ref(),
            self.state.occurrence.id(),
            &self.audio.progress(),
            std::time::Instant::now(),
            || {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            },
        );
        if update == ListeningUpdate::Started {
            self.state.song_selection.clear_scope(SongScope::History);
            if let Some(song) = song.as_ref() {
                self.ensure_song_cover(song, false);
            }
        }
        if update != ListeningUpdate::Unchanged {
            self.persist_listening_history();
        }
    }

    pub(in crate::app) fn finish_listening_event(&mut self, completed: bool) {
        if self.state.listening.finish(completed) {
            self.persist_listening_history();
        }
    }

    pub(in crate::app) fn persist_listening_history(&self) {
        let snapshot =
            config::CacheSnapshot::ListeningHistory(self.state.listening.history().clone());
        let submitted = match self.session.source() {
            Some(source) => self.server_cache_writer.submit_source(source, snapshot),
            None => self
                .server_cache_writer
                .submit(self.state.listening.server_id(), snapshot),
        };
        if let Err(error) = submitted {
            log::warn!("could not queue listening history: {error}");
        }
    }
}
