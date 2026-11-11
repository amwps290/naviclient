//! Shared player transport actions and manual stream retry.

use super::reconnect::PlaybackReconnect;
use super::start::StartIntent;
use crate::app::desktop::media_command_state::{playback_action, PlaybackAction, PlaybackRequest};
use crate::app::prelude::*;

pub(in crate::app) fn apply_playback_request(
    audio: &AudioHandle,
    request: PlaybackRequest,
    can_play: bool,
    start: impl FnOnce(),
) -> PlaybackAction {
    let action = playback_action(&audio.state(), request, can_play);
    match action {
        PlaybackAction::Start => start(),
        PlaybackAction::Resume => audio.resume(),
        PlaybackAction::Pause => audio.pause(),
        PlaybackAction::Unchanged => {}
    }
    action
}

fn apply_recovery_playback_request(
    audio: &AudioHandle,
    request: PlaybackRequest,
    intent: Option<StartIntent>,
    reconnect: &PlaybackReconnect,
) -> bool {
    if !intent.is_some_and(StartIntent::reconnecting) && !reconnect.pending() {
        return false;
    }
    match request {
        PlaybackRequest::Pause => audio.pause(),
        PlaybackRequest::Play => audio.resume(),
        PlaybackRequest::Toggle if audio.state().paused => audio.resume(),
        PlaybackRequest::Toggle => audio.pause(),
    }
    true
}

pub(in crate::app) fn stop_playback_state(
    state: &mut AppState,
    session: &mut SessionState,
) -> bool {
    state.cancel_deferred_playback();
    state.radio.cancel();
    let finished_listening = state.listening.finish(false);
    session.clear_position();
    state.now_playing = None;
    state.now_playing_quality = None;
    state.now_playing_audio_quality = None;
    state.lyrics.clear();
    state.occurrence.clear();
    finished_listening
}

impl NavidromeApp {
    /// 手动重试当前曲目：从出错位置重新拉流，并重置自动重连计数。
    pub(crate) fn retry_current_track(&mut self) {
        self.cancel_next_preload();
        let Some(index) = self.state.queue_index else {
            return;
        };
        self.state.cancel_deferred_playback();
        let position = self
            .playback_timeline()
            .map_or(Duration::ZERO, |timeline| timeline.position);
        log::info!(
            "manual playback retry requested; index={index} position_ms={}",
            position.as_millis()
        );
        self.reconnect.cancel();
        self.play_queue_index_at(index, position, StartIntent::User);
    }

    pub fn toggle_playback(&mut self) {
        self.request_playback(PlaybackRequest::Toggle, true);
    }

    pub(in crate::app) fn request_playback(&mut self, request: PlaybackRequest, can_play: bool) {
        let intent = self
            .start_resolution
            .value_mut()
            .map(|start| start.intent());
        if apply_recovery_playback_request(&self.audio, request, intent, &self.reconnect) {
            return;
        }
        self.start_resolution.cancel();
        self.state.occurrence.cancel_advance();
        self.state.cancel_deferred_playback();
        self.reconnect.cancel();
        self.sync_gapless_transition();
        let audio = self.audio.clone();
        apply_playback_request(&audio, request, can_play, || {
            if let Some(index) = self.state.queue_index {
                let position = self.session.start_position(self.state.now_playing.as_ref());
                self.play_queue_index_at(index, position, StartIntent::User);
            }
        });
    }

    pub fn stop_playback(&mut self) {
        self.start_resolution.cancel();
        self.lyrics_ui.cancel();
        self.reconnect.cancel();
        self.cancel_next_preload();
        self.audio.stop();
        if stop_playback_state(&mut self.state, &mut self.session) {
            self.persist_listening_history();
        }
        self.desktop.reset_track();
        self.track_notifier.cancel_pending();
    }
}

#[cfg(test)]
mod tests;
