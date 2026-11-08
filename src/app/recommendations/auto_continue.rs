//! Automatic continuation at the natural end of a sequential queue.

use crate::app::playback::occurrence::Selection;
use crate::app::playback::start::StartTarget;
use crate::app::prelude::*;
use crate::app::queue::logic::should_auto_continue;
use crate::recommendation::{generate_auto_continue, AutoContinueGenerationContext};

impl NavidromeApp {
    pub(in crate::app) fn set_auto_continue(&mut self, enabled: bool, cx: &mut Context<Self>) {
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

    pub(in crate::app) fn cancel_auto_continue(&mut self) {
        self.state.auto_continue.cancel();
    }

    pub(in crate::app) fn maybe_start_auto_continue(&mut self) {
        if self.state.auto_continue.loading()
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
        let Some(api) = self
            .playback_source
            .clone()
            .filter(|api| api.capabilities().recommendations)
        else {
            return;
        };
        let Some(server_id) = self.session.server_id().map(str::to_string) else {
            return;
        };
        let Some(request) = self.state.auto_continue.begin(
            &server_id,
            &self.state.queue,
            &mut self.library_requests,
        ) else {
            return;
        };
        let seeds = self
            .state
            .queue
            .songs()
            .rev()
            .take(3)
            .cloned()
            .collect::<Vec<_>>();
        let capabilities = self
            .playback_server
            .as_ref()
            .map(|server| server.capabilities)
            .unwrap_or_default();
        let common = self.recommendation_generation_context(
            server_id.clone(),
            capabilities.sonic_similarity,
            100,
        );
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result =
                generate_auto_continue(api, AutoContinueGenerationContext { common, seeds })
                    .await
                    .map_err(error_message);
            let _ = tx.send(Msg::AutoContinue { request, result });
        });
    }

    pub(in crate::app) fn finish_auto_continue(
        &mut self,
        request: AutoContinueRequest,
        result: Result<Vec<crate::recommendation::RecommendedTrack>, String>,
    ) {
        let Some(result) = self.state.auto_continue.complete(&request, result) else {
            return;
        };
        if !request.matches(
            self.session.server_id(),
            self.config.auto_continue,
            self.state.playback_modes,
            self.state.queue_index,
            &self.state.queue,
        ) {
            return;
        }

        match result {
            Ok(tracks) if !tracks.is_empty() => {
                let entries = tracks
                    .into_iter()
                    .map(|track| QueueEntry::new(track.song, track.origin))
                    .collect();
                let Some(target) = StartTarget::append(&self.state.queue, entries) else {
                    return;
                };
                let Some(start) =
                    self.prepare_playback_start(target, Duration::ZERO, Selection::New)
                else {
                    return;
                };
                if self.activate_playback_start(start) {
                    self.save_session();
                }
            }
            Ok(_) => log::debug!("automatic continuation returned no tracks"),
            Err(error) => {
                log::warn!("automatic continuation failed: {error}");
                self.notify_warning(t!("Automatic continuation could not find more music"));
            }
        }
    }
}
