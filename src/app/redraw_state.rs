//! Playback redraw cadence and cover animation eligibility without UI effects.

use std::time::{Duration, Instant};

use super::view_state::View;
use crate::audio::PlaybackState;

pub(super) const POLL_INTERVAL: Duration = Duration::from_millis(50);
const STANDARD_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlaybackPresentation {
    NowPlaying,
    Standard,
}

impl PlaybackPresentation {
    pub(super) fn for_view(view: View, settings_open: bool, mini: bool) -> Self {
        if view == View::NowPlaying && !settings_open && !mini {
            Self::NowPlaying
        } else {
            Self::Standard
        }
    }

    fn interval(self) -> Duration {
        match self {
            Self::NowPlaying => POLL_INTERVAL,
            Self::Standard => STANDARD_INTERVAL,
        }
    }

    pub(super) fn animate_cover(self, playback: &PlaybackState, visible: bool) -> bool {
        self == Self::NowPlaying && visible && playback.active && !playback.paused
    }
}

pub(super) struct RedrawRequest {
    pub(super) playback: PlaybackState,
    pub(super) presentation: PlaybackPresentation,
    pub(super) visible: bool,
    /// Messages, commands and ready animation frames also notify while hidden.
    pub(super) changed: bool,
    pub(super) scrolling: bool,
}

pub(super) struct PlaybackRedrawState {
    previous_playback: Option<PlaybackState>,
    playback_dirty: bool,
    last_render: Instant,
    was_visible: bool,
}

impl PlaybackRedrawState {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            previous_playback: None,
            playback_dirty: false,
            last_render: now,
            was_visible: false,
        }
    }

    pub(super) fn should_redraw(&mut self, now: Instant, request: RedrawRequest) -> bool {
        self.playback_dirty |= self.previous_playback.as_ref() != Some(&request.playback);
        let transport_changed = self
            .previous_playback
            .as_ref()
            .is_none_or(|last| transport_changed(last, &request.playback));
        self.previous_playback = Some(request.playback);

        // Keep pending playback changes across polls until a redraw, even if
        // later snapshots settle or the window is temporarily hidden.
        let redraw = request.changed
            || request.visible
                && (!self.was_visible
                    || transport_changed
                    || request.scrolling
                    || self.playback_dirty
                        && now.saturating_duration_since(self.last_render)
                            >= request.presentation.interval());
        if redraw {
            self.playback_dirty = false;
            self.last_render = now;
        }
        self.was_visible = request.visible;
        redraw
    }
}

fn transport_changed(last: &PlaybackState, current: &PlaybackState) -> bool {
    last.active != current.active
        || last.paused != current.paused
        || last.ended != current.ended
        || last.error != current.error
        || last.duration != current.duration
        || last.transition_id != current.transition_id
}

#[cfg(test)]
mod tests;
