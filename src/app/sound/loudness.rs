//! Applying playback loudness preferences and persisting local measurements.

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn song_gain(&self, song: &Song) -> f32 {
        if let Some(source) = song
            .source
            .as_ref()
            .filter(|source| Some(*source) != self.session.source())
        {
            let mut cache = crate::app::sound::loudness_state::LoudnessCache::default();
            cache.restore(
                Some(&source.profile_id),
                self.server_cache_writer
                    .pending_source_loudness(source)
                    .unwrap_or_else(|| config::load_source_loudness(source)),
            );
            return cache.gain(song, self.config.volume_normalization);
        }
        self.state
            .loudness
            .gain(song, self.config.volume_normalization)
    }

    pub(in crate::app) fn cache_measured_loudness(&mut self, rms_db: f32, peak: f32) {
        let Some(song_id) = self.state.now_playing.as_ref().map(|song| song.id.as_str()) else {
            return;
        };
        if !self
            .state
            .loudness
            .record(self.config.volume_normalization, song_id, rms_db, peak)
        {
            return;
        }
        self.persist_loudness();
        log::debug!(
            "measured loudness cached song={} rms_db={:.1} peak={:.3}",
            song_id,
            rms_db,
            peak,
        );
    }

    pub(in crate::app) fn persist_loudness(&self) {
        let snapshot = config::CacheSnapshot::Loudness(self.state.loudness.items().clone());
        let submitted = match self.session.source() {
            Some(source) => self.server_cache_writer.submit_source(source, snapshot),
            None => self
                .server_cache_writer
                .submit(self.state.loudness.server_id(), snapshot),
        };
        if let Err(error) = submitted {
            log::warn!("could not queue loudness cache: {error}");
        }
    }
}
