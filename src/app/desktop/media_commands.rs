//! Consume platform commands on the UI thread using shared playback decisions.

use super::media_command_state::{absolute_seek, relative_seek, PlaybackRequest, SeekAction};
use crate::app::prelude::*;

const COMMANDS_PER_POLL: usize = 32;

impl NavidromeApp {
    pub(in crate::app) fn poll_media_commands(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut changed = false;
        for _ in 0..COMMANDS_PER_POLL {
            let Some(command) = self.media_session.next_command() else {
                break;
            };
            changed = true;
            match command {
                MediaCommand::Play | MediaCommand::Pause | MediaCommand::PlayPause => {
                    let request = match command {
                        MediaCommand::Play => PlaybackRequest::Play,
                        MediaCommand::Pause => PlaybackRequest::Pause,
                        _ => PlaybackRequest::Toggle,
                    };
                    self.request_playback(request, self.media_capabilities().play);
                }
                MediaCommand::Stop => {
                    self.stop_playback();
                }
                MediaCommand::Next | MediaCommand::Previous => {
                    self.skip_media_track(matches!(command, MediaCommand::Next));
                }
                MediaCommand::Seek(offset) => {
                    // Cancelling can resolve a gapless handoff. Read the track
                    // and its bounds only after the UI has accepted that event.
                    self.cancel_next_preload();
                    let action = relative_seek(self.playback_timeline(), offset);
                    self.apply_media_seek(action);
                }
                MediaCommand::SetPosition { track_id, position } => {
                    self.cancel_next_preload();
                    let current_id = self.state.now_playing.as_ref().map(|song| {
                        media_session::track_id(
                            &self.playback_source_key(),
                            &song.id,
                            self.state.occurrence.id(),
                        )
                    });
                    let action = absolute_seek(
                        self.playback_timeline(),
                        current_id.as_deref(),
                        &track_id,
                        position,
                    );
                    self.apply_media_seek(action);
                }
                MediaCommand::Volume(volume) => {
                    self.volume_slider.update(cx, |slider, cx| {
                        slider.set_value(volume * 100.0, window, cx)
                    });
                    self.set_volume(volume, cx);
                }
                MediaCommand::Raise => window.activate_window(),
                MediaCommand::Quit => {
                    self.save_playback_before_exit();
                    self.quitting.store(true, Ordering::Relaxed);
                    cx.quit();
                    break;
                }
            }
        }
        changed
    }

    fn apply_media_seek(&mut self, action: SeekAction) {
        match action {
            SeekAction::Ignore => {}
            SeekAction::Seek(position) => self.seek_playback(position),
            SeekAction::Next => {
                // An explicit forward request cancels deferred playback even
                // when this track is already at the end of a non-looping queue.
                self.state.cancel_deferred_playback();
                self.skip_media_track(true);
            }
        }
    }

    fn skip_media_track(&mut self, next: bool) {
        if !self.media_capabilities().can_skip(next) {
            return;
        }
        self.state.cancel_deferred_playback();
        // Preserve the prepared stream while settling any already committed
        // handoff, then recheck the queue boundary for the selected track.
        self.freeze_next_preload();
        if self.media_capabilities().can_skip(next) {
            self.skip(if next { 1 } else { -1 });
        }
    }
}
