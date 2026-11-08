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
        if let Err(error) = &result {
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
        match result
            .map_err(|error| StartError::Request(anyhow::anyhow!(error.message)))
            .and_then(|resource| start.finish(resource))
        {
            Ok(start) => {
                self.commit_playback_start(start);
            }
            Err(error) => {
                self.cancel_next_preload();
                self.report_start_error(error);
            }
        }
    }

    pub(in crate::app) fn resolve_next_preload(&mut self, index: usize) {
        let Some(api) = self.playback_source.as_ref().and_then(|api| {
            if api.id().provider == crate::source::ProviderKind::Bilibili {
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
        Duration::from_secs(30),
        api.resolve_playback(song, configured),
    )
    .await
    .map_err(|_| PlaybackResolveError {
        message: t!("Audio resource resolution timed out. Retry playback.").to_string(),
        unavailable: false,
        login_expired: false,
    })?
    .map_err(|error| {
        use crate::source::bilibili::client::BilibiliError;
        let protocol = error.downcast_ref::<BilibiliError>();
        PlaybackResolveError {
            unavailable: matches!(
                protocol,
                Some(
                    BilibiliError::Unavailable
                        | BilibiliError::PermissionDenied
                        | BilibiliError::RegionRestricted
                        | BilibiliError::UnsupportedAudio
                )
            ),
            login_expired: matches!(protocol, Some(BilibiliError::NotAuthenticated)),
            message: error_message(error),
        }
    })
}
