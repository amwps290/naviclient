//! Recommendation context shared by daily mixes, radios and auto-continue.

use super::*;
use crate::recommendation::{ListeningProfile, RecommendationGenerationContext};

impl AppState {
    /// A newer manual playback choice replaces pending playback intents. The
    /// daily mix can still finish for its cache and card without starting audio.
    pub(super) fn cancel_deferred_playback(&mut self) {
        self.library_playback.cancel();
        self.auto_continue.cancel();
        self.daily_mix.cancel_playback();
    }
}

impl NavidromeApp {
    pub(super) fn recommendation_generation_context(
        &self,
        server_id: String,
        sonic_similarity: crate::models::CapabilitySupport,
        recent_limit: usize,
    ) -> RecommendationGenerationContext {
        let history = self.state.listening.history();
        RecommendationGenerationContext {
            server_id,
            sonic_similarity,
            recent_song_ids: history.recent_song_ids(recent_limit),
            listening_profile: ListeningProfile::from_history(history),
            excluded_song_ids: self
                .state
                .queue
                .songs()
                .map(|song| song.id.clone())
                .collect(),
        }
    }
}
