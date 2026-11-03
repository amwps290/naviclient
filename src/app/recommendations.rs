//! Daily mix orchestration: server-scoped cache, cancellable generation and
//! queue actions. Candidate fetching/ranking stays in `recommendation`.

use std::collections::HashSet;

use super::*;
use crate::recommendation::{
    current_day_key, generate_auto_continue, generate_daily_mix, AutoContinueGenerationContext,
    DailyMixGenerationContext, ListeningProfile,
};

impl NavidromeApp {
    pub(super) fn set_auto_continue(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.config.auto_continue == enabled {
            return;
        }
        self.config.auto_continue = enabled;
        if !enabled {
            self.cancel_auto_continue();
        }
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Automatic continuation save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        cx.notify();
    }

    pub(super) fn cancel_auto_continue(&mut self) {
        if self.auto_continue_loading {
            self.auto_continue_loading = false;
            self.auto_continue_generation = self.auto_continue_generation.wrapping_add(1);
        }
    }

    pub(super) fn maybe_start_auto_continue(&mut self) {
        if self.auto_continue_loading
            || !should_auto_continue(
                self.config.auto_continue,
                self.state.playback_modes,
                self.state.queue_index,
                self.state.queue.len(),
                true,
            )
        {
            return;
        }
        let Some(api) = self.api.clone() else { return };
        let Some(server_id) = self.config.active_server_id().map(str::to_string) else {
            return;
        };
        let Some(anchor_song_id) = self.state.queue.last().map(|song| song.id.clone()) else {
            return;
        };

        self.auto_continue_generation = self.auto_continue_generation.wrapping_add(1);
        let generation = self.auto_continue_generation;
        self.auto_continue_loading = true;
        let queue_len = self.state.queue.len();
        let seeds = self
            .state
            .queue
            .iter()
            .rev()
            .take(3)
            .cloned()
            .collect::<Vec<_>>();
        let capabilities = self
            .state
            .server
            .as_ref()
            .map(|server| server.capabilities)
            .unwrap_or_default();
        let recent_song_ids = self.state.listening_history.recent_song_ids(100);
        let listening_profile = ListeningProfile::from_history(&self.state.listening_history);
        let excluded_song_ids = self
            .state
            .queue
            .iter()
            .map(|song| song.id.clone())
            .collect::<HashSet<_>>();
        let tx = self.tx.clone();
        let message_server_id = server_id.clone();
        let message_anchor = anchor_song_id.clone();
        self.spawn_future(async move {
            let result = generate_auto_continue(
                api,
                AutoContinueGenerationContext {
                    server_id,
                    seeds,
                    sonic_similarity: capabilities.sonic_similarity,
                    recent_song_ids,
                    listening_profile,
                    excluded_song_ids,
                },
            )
            .await
            .map_err(error_message);
            let _ = tx.send(Msg::AutoContinue {
                generation,
                server_id: message_server_id,
                anchor_song_id: message_anchor,
                queue_len,
                result,
            });
        });
    }

    pub(super) fn finish_auto_continue(
        &mut self,
        generation: u64,
        server_id: String,
        anchor_song_id: String,
        queue_len: usize,
        result: Result<Vec<crate::recommendation::RecommendedTrack>, String>,
    ) {
        if generation != self.auto_continue_generation
            || self.config.active_server_id() != Some(server_id.as_str())
        {
            return;
        }
        self.auto_continue_loading = false;
        if !auto_continue_context_matches(
            self.config.auto_continue,
            self.state.playback_modes,
            self.state.queue_index,
            &self.state.queue,
            queue_len,
            &anchor_song_id,
        ) {
            return;
        }

        match result {
            Ok(tracks) if !tracks.is_empty() => {
                let first_new = self.state.queue.len();
                self.state.append_queue_with_origins(
                    tracks.iter().map(|track| track.song.clone()).collect(),
                    tracks.iter().map(|track| track.origin).collect(),
                );
                self.play_queue_index(first_new);
                self.save_session();
            }
            Ok(_) => log::debug!("automatic continuation returned no tracks"),
            Err(error) => {
                log::warn!("automatic continuation failed: {error}");
                self.notify_warning(t!("Automatic continuation could not find more music"));
            }
        }
    }

    pub(super) fn play_daily_mix(&mut self) {
        let day = current_day_key();
        let server_id = self.config.active_server_id().unwrap_or_default();
        let stale = self
            .state
            .daily_mix
            .as_ref()
            .is_some_and(|snapshot| !snapshot.is_current(server_id, &day));
        if stale {
            if let Some(snapshot) = self.state.daily_mix.take() {
                self.state.daily_mix_previous_ids = snapshot
                    .tracks
                    .into_iter()
                    .map(|track| track.song.id)
                    .collect();
            }
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
        let mut recent_song_ids = self.state.listening_history.recent_song_ids(50);
        recent_song_ids.extend(self.state.daily_mix_previous_ids.iter().cloned());
        if let Some(snapshot) = &self.state.daily_mix {
            recent_song_ids.extend(snapshot.tracks.iter().map(|track| track.song.id.clone()));
        }
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
                if let Some(previous) = &self.state.daily_mix {
                    self.state.daily_mix_previous_ids = previous
                        .tracks
                        .iter()
                        .map(|track| track.song.id.clone())
                        .collect();
                }
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
