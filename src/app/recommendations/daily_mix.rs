//! Daily mix generation, server-scoped cache, and queue actions.

use crate::app::prelude::*;
use crate::recommendation::{
    current_day_key, generate_daily_mix, DailyMixGenerationContext, DailyMixSnapshot,
};

impl NavidromeApp {
    pub(in crate::app) fn play_daily_mix(&mut self) {
        self.cancel_recommendation_tasks();
        let day = current_day_key();
        let server_id = self.config.active_server_id().unwrap_or_default();
        self.state.daily_mix.expire(server_id, &day);
        if let Some(snapshot) = self.state.daily_mix.snapshot() {
            let songs = snapshot.songs();
            if !songs.is_empty() {
                self.replace_queue_and_play(songs, 0, QueueOrigin::DailyMix);
            }
        } else {
            self.generate_daily_mix(0, true);
        }
    }

    pub(in crate::app) fn append_daily_mix(&mut self) {
        if let Some(snapshot) = self.state.daily_mix.snapshot() {
            let existing = self
                .state
                .queue
                .songs()
                .filter(|song| self.browsing_owns_song(song))
                .map(|song| song.id.as_str())
                .collect::<HashSet<_>>();
            let songs = snapshot
                .tracks
                .iter()
                .filter(|track| !existing.contains(track.song.id.as_str()))
                .map(|track| track.song.clone())
                .collect::<Vec<_>>();
            self.append_all_with_origin(songs, QueueOrigin::DailyMix);
        }
    }

    pub(in crate::app) fn refresh_daily_mix(&mut self) {
        let day = current_day_key();
        let server_id = self.config.active_server_id().unwrap_or_default();
        let variant = self.state.daily_mix.refresh_variant(server_id, &day);
        self.generate_daily_mix(variant, false);
    }

    pub(in crate::app) fn retry_daily_mix(&mut self) {
        let variant = self.state.daily_mix.retry_variant();
        let play_when_ready = self.state.daily_mix.snapshot().is_none();
        self.generate_daily_mix(variant, play_when_ready);
    }

    fn generate_daily_mix(&mut self, variant: u32, play_when_ready: bool) {
        let Some(api) = self.api.clone() else {
            self.notify_error(t!("Configure a server before playing"));
            return;
        };
        let Some(server_id) = self.config.active_server_id().map(str::to_string) else {
            self.notify_error(t!("Configure a server before playing"));
            return;
        };
        let day = current_day_key();
        let Some(request) = self.state.daily_mix.begin(
            &server_id,
            &day,
            variant,
            play_when_ready,
            &mut self.library_requests,
        ) else {
            return;
        };
        let sonic_similarity = self
            .state
            .connection
            .server()
            .map(|server| server.capabilities.sonic_similarity)
            .unwrap_or_default();
        let mut common =
            self.recommendation_generation_context(server_id.clone(), sonic_similarity, 50);
        common
            .recent_song_ids
            .extend(self.state.daily_mix.recent_song_ids().cloned());
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = generate_daily_mix(
                api,
                DailyMixGenerationContext {
                    common,
                    day,
                    variant,
                },
            )
            .await
            .map_err(error_message);
            let _ = tx.send(Msg::DailyMix { request, result });
        });
    }

    pub(in crate::app) fn finish_daily_mix(
        &mut self,
        request: DailyMixRequest,
        result: Result<DailyMixSnapshot, String>,
    ) {
        if self.config.active_server_id() != Some(request.server_id()) {
            return;
        }
        let Some(Ok(play_when_ready)) = self.state.daily_mix.complete(&request, result) else {
            return;
        };
        let snapshot = self
            .state
            .daily_mix
            .snapshot()
            .expect("accepted daily mix exists");
        if let Err(error) = self.server_cache_writer.submit(
            Some(request.server_id()),
            config::CacheSnapshot::DailyMix(snapshot.clone()),
        ) {
            log::warn!("could not queue daily mix cache: {error}");
        }
        let accent_cover = snapshot
            .tracks
            .iter()
            .find_map(|track| track.song.cover_art.clone());
        let covers = snapshot
            .tracks
            .iter()
            .filter_map(|track| track.song.cover_art.clone())
            .take(30)
            .collect();
        self.ensure_cover(accent_cover.as_deref(), true);
        self.preload_covers(covers);
        if play_when_ready {
            self.play_daily_mix();
        }
    }
}
