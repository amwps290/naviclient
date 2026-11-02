//! Daily mix orchestration: server-scoped cache, cancellable generation and
//! queue actions. Candidate fetching/ranking stays in `recommendation`.

use std::collections::HashSet;

use super::*;
use crate::recommendation::{
    current_day_key, generate_daily_mix, DailyMixGenerationContext, ListeningProfile,
};

impl NavidromeApp {
    pub(super) fn play_daily_mix(&mut self) {
        let day = current_day_key();
        let server_id = self.config.active_server_id().unwrap_or_default();
        if self
            .state
            .daily_mix
            .as_ref()
            .is_some_and(|snapshot| !snapshot.is_current(server_id, &day))
        {
            self.state.daily_mix = None;
        }
        if let Some(snapshot) = &self.state.daily_mix {
            let songs = snapshot.songs();
            if !songs.is_empty() {
                self.play_song_list_with_origin(&songs, 0, QueueOrigin::DailyMix);
            }
        } else {
            self.generate_daily_mix(0, true);
        }
    }

    pub(super) fn append_daily_mix(&mut self) {
        if let Some(snapshot) = &self.state.daily_mix {
            let existing = self
                .state
                .queue
                .iter()
                .map(|song| song.id.as_str())
                .collect::<HashSet<_>>();
            let songs = snapshot
                .tracks
                .iter()
                .filter(|track| !existing.contains(track.song.id.as_str()))
                .map(|track| track.song.clone())
                .collect::<Vec<_>>();
            self.append_all_with_origin(&songs, QueueOrigin::DailyMix);
        }
    }

    pub(super) fn refresh_daily_mix(&mut self) {
        let day = current_day_key();
        let server_id = self.config.active_server_id().unwrap_or_default();
        let variant = self.state.daily_mix.as_ref().map_or(0, |snapshot| {
            if snapshot.is_current(server_id, &day) {
                snapshot.variant.wrapping_add(1)
            } else {
                0
            }
        });
        self.generate_daily_mix(variant, false);
    }

    pub(super) fn retry_daily_mix(&mut self) {
        let variant = self
            .state
            .daily_mix
            .as_ref()
            .map_or(0, |snapshot| snapshot.variant);
        let play_when_ready = self.state.daily_mix.is_none();
        self.generate_daily_mix(variant, play_when_ready);
    }

    fn generate_daily_mix(&mut self, variant: u32, play_when_ready: bool) {
        if self.state.daily_mix_loading {
            self.state.daily_mix_play_when_ready |= play_when_ready;
            return;
        }
        let Some(api) = self.api.clone() else {
            self.notify_error(t!("Configure a server before playing"));
            return;
        };
        let Some(server_id) = self.config.active_server_id().map(str::to_string) else {
            self.notify_error(t!("Configure a server before playing"));
            return;
        };
        self.daily_mix_generation = self.daily_mix_generation.wrapping_add(1);
        let generation = self.daily_mix_generation;
        self.state.daily_mix_loading = true;
        self.state.daily_mix_error = None;
        self.state.daily_mix_play_when_ready = play_when_ready;

        let day = current_day_key();
        let sonic_similarity = self
            .state
            .server
            .as_ref()
            .map(|server| server.capabilities.sonic_similarity)
            .unwrap_or_default();
        let recent_song_ids = self.state.listening_history.recent_song_ids(50);
        let listening_profile = ListeningProfile::from_history(&self.state.listening_history);
        let excluded_song_ids = self
            .state
            .queue
            .iter()
            .map(|song| song.id.clone())
            .collect::<HashSet<_>>();
        let tx = self.tx.clone();
        let message_server_id = server_id.clone();
        self.spawn_future(async move {
            let result = generate_daily_mix(
                api,
                DailyMixGenerationContext {
                    server_id,
                    day,
                    variant,
                    sonic_similarity,
                    recent_song_ids,
                    listening_profile,
                    excluded_song_ids,
                },
            )
            .await
            .map_err(error_message);
            let _ = tx.send(Msg::DailyMix {
                generation,
                server_id: message_server_id,
                result,
            });
        });
    }

    pub(super) fn finish_daily_mix(
        &mut self,
        generation: u64,
        server_id: String,
        result: Result<DailyMixSnapshot, String>,
    ) {
        if generation != self.daily_mix_generation
            || self.config.active_server_id() != Some(server_id.as_str())
        {
            return;
        }
        self.state.daily_mix_loading = false;
        match result {
            Ok(snapshot) => {
                if let Err(error) = config::save_daily_mix(Some(&server_id), &snapshot) {
                    log::warn!("daily mix cache save failed: {error:#}");
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
                let play_when_ready = self.state.daily_mix_play_when_ready;
                self.state.daily_mix_play_when_ready = false;
                self.state.daily_mix_error = None;
                self.state.daily_mix = Some(snapshot);
                self.ensure_cover(accent_cover.as_deref(), true);
                self.preload_covers(covers);
                if play_when_ready {
                    self.play_daily_mix();
                }
            }
            Err(error) => {
                self.state.daily_mix_play_when_ready = false;
                self.state.daily_mix_error = Some(error);
            }
        }
    }
}
