//! Mute memory and delayed volume interactions, independent of windows and audio.

use std::time::{Duration, Instant};

pub(super) const VOLUME_SAVE_DELAY: Duration = Duration::from_millis(500);
pub(super) const VOLUME_PANEL_CLOSE_DELAY: Duration = Duration::from_millis(220);
pub(super) const MINI_VOLUME_FLASH_CLEAR_DELAY: Duration = Duration::from_millis(1100);
const MINI_VOLUME_FLASH_DURATION: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct VolumeSaveRequest(u64);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct VolumePanelCloseRequest(u64);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct MiniVolumeFlashExpiry(u64);

pub(super) struct VolumeState {
    muted: bool,
    before_mute: f32,
    save_generation: u64,
    pending_save: Option<VolumeSaveRequest>,
    panel_open: bool,
    panel_dragging: bool,
    panel_generation: u64,
    pending_panel_close: Option<VolumePanelCloseRequest>,
    flash_generation: u64,
    mini_flash: Option<(MiniVolumeFlashExpiry, Instant)>,
}

impl VolumeState {
    pub(super) fn new(saved_volume: f32) -> Self {
        Self {
            muted: false,
            before_mute: restored_volume(saved_volume),
            save_generation: 0,
            pending_save: None,
            panel_open: false,
            panel_dragging: false,
            panel_generation: 0,
            pending_panel_close: None,
            flash_generation: 0,
            mini_flash: None,
        }
    }

    pub(super) fn muted(&self) -> bool {
        self.muted
    }

    pub(super) fn output_volume(&self, saved_volume: f32) -> f32 {
        effective_volume(saved_volume, self.muted)
    }

    /// Config remains the owner of the saved volume, including while muted.
    pub(super) fn set_volume(&mut self, saved_volume: &mut f32, volume: f32) -> f32 {
        let volume = normalize_volume(volume);
        self.muted = false;
        *saved_volume = volume;
        if volume > 0.001 {
            self.before_mute = volume;
        }
        volume
    }

    /// Returns the new audio output; muting leaves the saved volume untouched.
    pub(super) fn toggle_mute(&mut self, saved_volume: &mut f32) -> f32 {
        if self.muted || *saved_volume <= 0.001 {
            self.set_volume(saved_volume, restored_volume(self.before_mute))
        } else {
            self.before_mute = *saved_volume;
            self.muted = true;
            0.0
        }
    }

    pub(super) fn begin_save(&mut self) -> VolumeSaveRequest {
        self.save_generation = self.save_generation.wrapping_add(1);
        let request = VolumeSaveRequest(self.save_generation);
        self.pending_save = Some(request);
        request
    }

    pub(super) fn complete_save(&mut self, request: VolumeSaveRequest) -> bool {
        if self.pending_save != Some(request) {
            return false;
        }
        self.pending_save = None;
        true
    }

    pub(super) fn panel_open(&self) -> bool {
        self.panel_open
    }

    pub(super) fn show_panel(&mut self) {
        self.pending_panel_close = None;
        self.panel_open = true;
    }

    pub(super) fn request_panel_close(&mut self) -> VolumePanelCloseRequest {
        self.panel_generation = self.panel_generation.wrapping_add(1);
        let request = VolumePanelCloseRequest(self.panel_generation);
        self.pending_panel_close = Some(request);
        request
    }

    pub(super) fn complete_panel_close(&mut self, request: VolumePanelCloseRequest) -> bool {
        if self.pending_panel_close != Some(request) || self.panel_dragging {
            return false;
        }
        self.pending_panel_close = None;
        self.panel_open = false;
        true
    }

    pub(super) fn begin_panel_drag(&mut self) {
        self.panel_dragging = true;
        self.show_panel();
    }

    pub(super) fn end_panel_drag(&mut self, keep_open: bool) -> Option<VolumePanelCloseRequest> {
        self.panel_dragging = false;
        if keep_open {
            self.show_panel();
            None
        } else {
            Some(self.request_panel_close())
        }
    }

    pub(super) fn show_mini_flash(&mut self, now: Instant) -> MiniVolumeFlashExpiry {
        self.flash_generation = self.flash_generation.wrapping_add(1);
        let expiry = MiniVolumeFlashExpiry(self.flash_generation);
        self.mini_flash = Some((expiry, now));
        expiry
    }

    pub(super) fn mini_flash_visible(&self, now: Instant) -> bool {
        self.mini_flash
            .is_some_and(|(_, at)| now.duration_since(at) < MINI_VOLUME_FLASH_DURATION)
    }

    pub(super) fn expire_mini_flash(&mut self, expiry: MiniVolumeFlashExpiry) -> bool {
        if !self
            .mini_flash
            .is_some_and(|(current, _)| current == expiry)
        {
            return false;
        }
        self.mini_flash = None;
        true
    }
}

pub(super) fn normalize_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        0.7
    }
}

fn effective_volume(volume: f32, muted: bool) -> f32 {
    if muted {
        0.0
    } else {
        normalize_volume(volume)
    }
}

fn restored_volume(volume_before_mute: f32) -> f32 {
    let volume = normalize_volume(volume_before_mute);
    if volume > 0.001 {
        volume
    } else {
        0.7
    }
}

#[cfg(test)]
mod tests;
