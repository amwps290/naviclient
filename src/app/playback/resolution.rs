//! Async provider work is separate from committing the player and its queue.

use super::resolution_state::PlaybackResolveRequest;
use super::start::{PlaybackRequest, StartError, UnresolvedStart};
use crate::app::prelude::*;
use crate::models::QueueEntryId;
use crate::msg::PlaybackResolveError;

pub(in crate::app) struct PreloadPlan {
    index: usize,
    entry: QueueEntryId,
    song: Song,
    configured: TranscodingQuality,
}

impl NavidromeApp {
    pub(in crate::app) fn resolve_playback_start(&mut self, start: UnresolvedStart) {
        self.freeze_next_preload();
        let api = start.source.clone();
        let song = start.song().clone();
        let configured = start.configured();
        let request = self.start_resolution.begin(
            song.media_key().expect("start binds its source"),
            self.state.occurrence.id(),
            start,
            &mut self.library_requests,
        );
        let tx = self.tx.clone();
        let ticket = request.clone();
        let task = self.runtime.spawn(async move {
            let result = resolve_with_timeout(&api, &song, configured).await;
            let _ = tx.send(Msg::PlaybackResolved {
                request: ticket,
                result,
            });
        });
        self.start_resolution.attach(&request, task.abort_handle());
    }

    pub(in crate::app) fn finish_playback_resolution(
        &mut self,
        request: PlaybackResolveRequest,
        result: Result<crate::source::PlaybackResource, PlaybackResolveError>,
    ) {
        self.sync_gapless_transition();
        let Some(start) = self
            .start_resolution
            .complete(&request, self.state.occurrence.id())
        else {
            return;
        };
        // Settle the old track before committing this accepted intent. Keep
        // advancement deferred until its success or failure is known.
        self.handle_playback_end_for_intent(true);
        start.settle_intent(&mut self.state.occurrence);
        if let Err(error) = &result {
            if start.intent().reconnecting() {
                let decision = self.reconnect.resolution_failed(
                    self.state.occurrence.id(),
                    error.retry_failure,
                    std::time::Instant::now(),
                );
                if self.report_reconnect_decision(
                    error.retry_failure,
                    decision,
                    PLAYBACK_RECONNECT_ATTEMPTS,
                    self.audio.progress().position,
                ) {
                    return;
                }
            }
            if error.login_expired {
                self.expire_bilibili_account(start.source.id());
            }
            if error.unavailable {
                let message = error.message.clone();
                if let Some(next) = start.skip_unavailable(&self.state.queue) {
                    self.notify_warning(t!("Skipping unavailable Bilibili audio"));
                    self.resolve_playback_start(next);
                } else {
                    self.report_start_error(StartError::Request(anyhow::anyhow!(message)));
                }
                return;
            }
        }
        let reconnecting = start.intent().reconnecting();
        match result
            .map_err(|error| StartError::Request(anyhow::anyhow!(error.message)))
            .and_then(|resource| start.finish(resource))
        {
            Ok(start) => {
                if !self.commit_playback_start(start) && reconnecting {
                    self.reconnect.cancel();
                }
            }
            Err(error) => {
                self.cancel_next_preload();
                if reconnecting {
                    self.reconnect.cancel();
                }
                self.report_start_error(error);
            }
        }
    }

    pub(in crate::app) fn resolve_next_preload(&mut self, index: usize) {
        let Some(api) = self.playback_source.as_ref().and_then(|api| {
            if api.playback_policy().authorized_session {
                self.available_source(api.id())
            } else {
                Some(api.clone())
            }
        }) else {
            return;
        };
        let Some(entry) = self.state.queue.entry_id(index) else {
            return;
        };
        let Some(song) = self.state.queue.song(index).cloned() else {
            return;
        };
        if !song.belongs_to(api.id()) {
            return;
        }
        let configured = self.config.transcoding_quality;
        let media = crate::source::MediaKey::new(api.id().clone(), &song.id);
        let request = self.preload_resolution.begin(
            media,
            self.state.occurrence.id(),
            PreloadPlan {
                index,
                entry,
                song: song.clone(),
                configured,
            },
            &mut self.library_requests,
        );
        let tx = self.tx.clone();
        let ticket = request.clone();
        let task = self.runtime.spawn(async move {
            let result = resolve_with_timeout(&api, &song, configured).await;
            let _ = tx.send(Msg::PreloadResolved {
                request: ticket,
                result,
            });
        });
        self.preload_resolution
            .attach(&request, task.abort_handle());
    }

    pub(in crate::app) fn finish_preload_resolution(
        &mut self,
        request: PlaybackResolveRequest,
        result: Result<crate::source::PlaybackResource, PlaybackResolveError>,
    ) {
        self.sync_gapless_transition();
        let Some(plan) = self
            .preload_resolution
            .complete(&request, self.state.occurrence.id())
        else {
            return;
        };
        if self.state.queue.entry_id(plan.index) != Some(plan.entry)
            || !self.state.occurrence.active()
            || self.start_resolution.pending()
        {
            return;
        }
        if result.as_ref().is_err_and(|error| error.login_expired) {
            if let Some(source) = &plan.song.source {
                self.expire_bilibili_account(source);
                self.notify_error(t!("Bilibili login expired. Sign in again."));
            }
            return;
        }
        let Ok(request) = result
            .map_err(|error| anyhow::Error::msg(error.message))
            .and_then(|resource| {
                PlaybackRequest::from_resource(resource, &plan.song, plan.configured)
            })
        else {
            return;
        };
        let gain = self.song_gain(&plan.song);
        let audio = self.audio.clone();
        let cache_key = request.cache_key.clone();
        if self
            .preload
            .reserve(&self.state.queue, plan.index, request.quality, |id| {
                audio.preload(id, request.url, request.cache_key, request.duration, gain)
            })
        {
            self.preload.set_resource(request.audio_quality, cache_key);
        }
    }
}

async fn resolve_with_timeout(
    api: &MusicSource,
    song: &Song,
    configured: TranscodingQuality,
) -> Result<crate::source::PlaybackResource, PlaybackResolveError> {
    tokio::time::timeout(
        crate::source::resource::timing::START_RESOLUTION_TIMEOUT,
        api.resolve_playback(song, configured),
    )
    .await
    .map_err(|_| PlaybackResolveError {
        message: t!("Audio resource resolution timed out. Retry playback.").to_string(),
        unavailable: false,
        login_expired: false,
        retry_failure: Some(StreamFailure::Timeout),
    })?
    .map_err(resolution_error)
}

fn resolution_error(error: anyhow::Error) -> PlaybackResolveError {
    use crate::source::playback::{classify_resolution_error, ResolutionFailure};
    let failure = classify_resolution_error(&error);
    PlaybackResolveError {
        unavailable: failure == ResolutionFailure::Unavailable,
        login_expired: failure == ResolutionFailure::SessionExpired,
        retry_failure: match failure {
            ResolutionFailure::Network => Some(StreamFailure::Connect),
            ResolutionFailure::Timeout => Some(StreamFailure::Timeout),
            ResolutionFailure::Http(status) if status == 408 || status >= 500 => {
                Some(StreamFailure::Http(status))
            }
            // Risk control, authentication and authoritative item failures
            // must not become a rapid automatic API retry loop.
            _ => None,
        },
        message: error_message(error),
    }
}

#[cfg(test)]
mod tests;
