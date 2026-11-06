//! Playback-mode edits, mode-only persistence, and shared UI coordination.

use gpui_kit::Context;
use rust_i18n::t;

use crate::app::NavidromeApp;
use crate::config;
use crate::models::{Config, PlayModes, RepeatMode};

#[derive(Clone, Copy)]
pub(in crate::app) enum PlaybackModeEdit {
    ToggleShuffle,
    CycleRepeat,
    AllowContinuation,
}

impl PlaybackModeEdit {
    fn next(self, mut modes: PlayModes) -> Option<PlayModes> {
        match self {
            Self::ToggleShuffle => modes.shuffle = !modes.shuffle,
            Self::CycleRepeat => modes.repeat = modes.repeat.next(),
            Self::AllowContinuation if modes.repeat == RepeatMode::One => {
                modes.repeat = RepeatMode::Off;
            }
            Self::AllowContinuation => return None,
        }
        Some(modes)
    }

    /// Runtime modes are authoritative. A save failure keeps the requested
    /// modes in memory, matching the UI and the next session snapshot.
    fn save(
        self,
        modes: &mut PlayModes,
        config: &mut Config,
        save: impl FnOnce(&Config) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let Some(next) = self.next(*modes) else {
            return Ok(());
        };
        *modes = next;
        config.playback_modes = next;
        save(config)
    }
}

impl NavidromeApp {
    pub(in crate::app) fn toggle_shuffle(&mut self, cx: &mut Context<Self>) {
        self.change_playback_mode(PlaybackModeEdit::ToggleShuffle, cx);
    }

    pub(in crate::app) fn cycle_repeat(&mut self, cx: &mut Context<Self>) {
        self.change_playback_mode(PlaybackModeEdit::CycleRepeat, cx);
    }

    fn change_playback_mode(&mut self, edit: PlaybackModeEdit, cx: &mut Context<Self>) {
        self.cancel_auto_continue();
        self.cancel_next_preload();
        self.apply_playback_mode_edit(edit);
        self.save_session();
        cx.notify();
    }

    /// Radio startup already owns its cancellation and session-save ordering.
    pub(in crate::app) fn apply_playback_mode_edit(&mut self, edit: PlaybackModeEdit) {
        if let Err(error) = edit.save(
            &mut self.state.playback_modes,
            &mut self.config,
            config::save,
        ) {
            self.notify_error(t!(
                "Playback mode save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
    }
}

#[cfg(test)]
mod tests;
