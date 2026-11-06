//! Canonical equalizer edits and single-use debounced persistence requests.

use std::time::Duration;

use crate::models::EqSettings;

pub(super) const EQUALIZER_SAVE_DELAY: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct EqualizerSaveRequest(u64);

#[derive(Default)]
pub(super) struct EqualizerState {
    save_generation: u64,
    pending_save: Option<EqualizerSaveRequest>,
}

impl EqualizerState {
    pub(super) fn set_band(
        &mut self,
        settings: &mut EqSettings,
        band: usize,
        gain_db: f32,
    ) -> Option<EqualizerSaveRequest> {
        let mut updated = settings.clone();
        *updated.gains_db.get_mut(band)? = gain_db;
        updated.enabled = true;
        self.apply(settings, updated)
    }

    pub(super) fn set_enabled(
        &mut self,
        settings: &mut EqSettings,
        enabled: bool,
    ) -> Option<EqualizerSaveRequest> {
        let mut updated = settings.clone();
        updated.enabled = enabled;
        self.apply(settings, updated)
    }

    pub(super) fn set_preset(
        &mut self,
        settings: &mut EqSettings,
        gains_db: [f32; 10],
    ) -> Option<EqualizerSaveRequest> {
        self.apply(
            settings,
            EqSettings {
                enabled: true,
                gains_db,
            },
        )
    }

    /// Config owns the settings; changes produce a save request only after normalization.
    fn apply(
        &mut self,
        settings: &mut EqSettings,
        mut updated: EqSettings,
    ) -> Option<EqualizerSaveRequest> {
        updated.clamp_gains();
        if *settings == updated {
            return None;
        }
        *settings = updated;
        self.save_generation = self.save_generation.wrapping_add(1);
        let request = EqualizerSaveRequest(self.save_generation);
        self.pending_save = Some(request);
        Some(request)
    }

    pub(super) fn complete_save(&mut self, request: EqualizerSaveRequest) -> bool {
        if self.pending_save != Some(request) {
            return false;
        }
        self.pending_save = None;
        true
    }
}

#[cfg(test)]
mod tests;
