//! External media command decisions without audio, window, or platform calls.

use std::time::Duration;

use super::queue_logic::advance_index;
use crate::audio::PlaybackState;
use crate::models::{PlayModes, RepeatMode};
use crate::playback_timeline::PlaybackTimeline;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct MediaCapabilities {
    pub(super) play: bool,
    pub(super) next: bool,
    pub(super) previous: bool,
}

impl MediaCapabilities {
    pub(super) fn for_queue(
        connected: bool,
        index: Option<usize>,
        len: usize,
        modes: PlayModes,
        played_history_len: usize,
    ) -> Self {
        let Some(index) = index.filter(|index| connected && *index < len) else {
            return Self::default();
        };
        let wraps = modes.repeat != RepeatMode::Off;
        let (next, previous) = if modes.shuffle {
            (true, played_history_len > 1)
        } else {
            (
                advance_index(index, len, true, wraps).is_some(),
                advance_index(index, len, false, wraps).is_some(),
            )
        };
        Self {
            play: true,
            next,
            previous,
        }
    }

    pub(super) fn can_skip(self, next: bool) -> bool {
        if next {
            self.next
        } else {
            self.previous
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum PlaybackRequest {
    Play,
    Pause,
    Toggle,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PlaybackAction {
    Unchanged,
    Start,
    Resume,
    Pause,
}

/// The audio handle includes accepted commands that the worker has not yet
/// processed. Decisions read that snapshot without recording a second intent.
pub(super) fn playback_action(
    playback: &PlaybackState,
    request: PlaybackRequest,
    can_play: bool,
) -> PlaybackAction {
    let play = match request {
        PlaybackRequest::Play => true,
        PlaybackRequest::Pause => false,
        PlaybackRequest::Toggle => !playback.active || playback.paused,
    };
    match (play, can_play, playback.active, playback.paused) {
        (true, true, false, _) => PlaybackAction::Start,
        (true, true, true, true) => PlaybackAction::Resume,
        (false, _, true, false) => PlaybackAction::Pause,
        _ => PlaybackAction::Unchanged,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SeekAction {
    Ignore,
    Seek(Duration),
    Next,
}

pub(super) fn relative_seek(context: Option<PlaybackTimeline>, offset_us: i64) -> SeekAction {
    let Some(context) = context else {
        return SeekAction::Ignore;
    };
    if offset_us > 0
        && context.duration.is_some_and(|limit| {
            context
                .position
                .saturating_add(Duration::from_micros(offset_us as u64))
                > limit
        })
    {
        return SeekAction::Next;
    }
    SeekAction::Seek(context.seek_micros(offset_us))
}

pub(super) fn absolute_seek(
    context: Option<PlaybackTimeline>,
    current_track_id: Option<&str>,
    track_id: &str,
    position: Duration,
) -> SeekAction {
    match context {
        Some(context)
            if current_track_id == Some(track_id)
                && context.duration.is_none_or(|limit| position <= limit) =>
        {
            SeekAction::Seek(position)
        }
        _ => SeekAction::Ignore,
    }
}

#[cfg(test)]
mod tests;
