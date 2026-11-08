//! Recommendation context shared by daily mixes, radios and auto-continue.

use crate::app::prelude::*;
use crate::recommendation::{ListeningProfile, RecommendationGenerationContext};

impl AppState {
    /// A newer manual playback choice replaces pending playback intents. The
    /// daily mix can still finish for its cache and card without starting audio.
    pub(in crate::app) fn cancel_deferred_playback(&mut self) {
        self.library_playback.cancel();
        self.auto_continue.cancel();
        self.daily_mix.cancel_playback();
    }
}

impl NavidromeApp {
    pub(in crate::app) fn recommendation_generation_context(
        &self,
        server_id: String,
        sonic_similarity: crate::models::CapabilitySupport,
        recent_limit: usize,
    ) -> RecommendationGenerationContext {
        let owner =
            crate::source::SourceId::new(crate::source::ProviderKind::OpenSubsonic, &server_id);
        let other_history;
        let history = if self.session.source() == Some(&owner) {
            self.state.listening.history()
        } else {
            other_history = self
                .server_cache_writer
                .pending_source_history(&owner)
                .unwrap_or_else(|| config::load_source_history(&owner));
            &other_history
        };
        RecommendationGenerationContext {
            server_id,
            sonic_similarity,
            recent_song_ids: history.recent_song_ids(recent_limit),
            listening_profile: ListeningProfile::from_history(history),
            excluded_song_ids: self
                .state
                .queue
                .songs()
                .filter(|song| song.belongs_to(&owner))
                .map(|song| song.id.clone())
                .collect(),
        }
    }
}
