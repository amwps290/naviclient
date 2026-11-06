//! Local listening observation and server-scoped persistence.

use super::listening_state::ListeningUpdate;
use super::*;

impl NavidromeApp {
    pub(super) fn poll_listening_history(&mut self) {
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
            self.ensure_cover(
                song.as_ref().and_then(|song| song.cover_art.as_deref()),
                false,
            );
        }
        if update != ListeningUpdate::Unchanged {
            self.persist_listening_history();
        }
    }

    pub(super) fn finish_listening_event(&mut self, completed: bool) {
        if self.state.listening.finish(completed) {
            self.persist_listening_history();
        }
    }

    pub(super) fn persist_listening_history(&self) {
        if let Err(error) = self.server_cache_writer.submit(
            self.state.listening.server_id(),
            config::CacheSnapshot::ListeningHistory(self.state.listening.history().clone()),
        ) {
            log::warn!("could not queue listening history: {error}");
        }
    }
}
