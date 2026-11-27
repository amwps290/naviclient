//! Canonical equalizer edits and single-use debounced persistence requests.

use std::time::Duration;

use crate::models::{CustomEqPreset, EqSettings};

pub(in crate::app) const EQUALIZER_SAVE_DELAY: Duration = Duration::from_millis(500);
pub(in crate::app) const MAX_CUSTOM_EQ_PRESETS: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) struct EqualizerSaveRequest(u64);

#[derive(Default)]
pub(in crate::app) struct EqualizerState {
    save_generation: u64,
    pending_save: Option<EqualizerSaveRequest>,
}

impl EqualizerState {
    pub(in crate::app) fn set_band(
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

    pub(in crate::app) fn set_preamp(
        &mut self,
        settings: &mut EqSettings,
        preamp_db: f32,
    ) -> Option<EqualizerSaveRequest> {
        let mut updated = settings.clone();
        updated.preamp_db = preamp_db;
        updated.enabled = true;
        self.apply(settings, updated)
    }

    pub(in crate::app) fn set_enabled(
        &mut self,
        settings: &mut EqSettings,
        enabled: bool,
    ) -> Option<EqualizerSaveRequest> {
        let mut updated = settings.clone();
        updated.enabled = enabled;
        self.apply(settings, updated)
    }

    #[cfg(test)]
    pub(in crate::app) fn set_preset(
        &mut self,
        settings: &mut EqSettings,
        gains_db: [f32; 10],
    ) -> Option<EqualizerSaveRequest> {
        self.set_preset_with_preamp(settings, 0.0, gains_db)
    }

    pub(in crate::app) fn set_preset_with_preamp(
        &mut self,
        settings: &mut EqSettings,
        preamp_db: f32,
        gains_db: [f32; 10],
    ) -> Option<EqualizerSaveRequest> {
        let mut updated = settings.clone();
        updated.enabled = true;
        updated.preamp_db = preamp_db;
        updated.gains_db = gains_db;
        self.apply(settings, updated)
    }

    pub(in crate::app) fn save_custom_preset(
        &mut self,
        settings: &mut EqSettings,
        name: &str,
    ) -> Option<EqualizerSaveRequest> {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return None;
        }
        let mut updated = settings.clone();
        let entry = CustomEqPreset {
            name: trimmed.to_string(),
            preamp_db: updated.preamp_db,
            gains_db: updated.gains_db,
        };
        if let Some(existing) = updated
            .custom_presets
            .iter_mut()
            .find(|preset| preset.name.eq_ignore_ascii_case(trimmed))
        {
            *existing = entry;
        } else if updated.custom_presets.len() < MAX_CUSTOM_EQ_PRESETS {
            updated.custom_presets.push(entry);
        } else {
            return None;
        }
        self.apply(settings, updated)
    }

    pub(in crate::app) fn delete_custom_preset(
        &mut self,
        settings: &mut EqSettings,
        name: &str,
    ) -> Option<EqualizerSaveRequest> {
        let mut updated = settings.clone();
        let before = updated.custom_presets.len();
        updated.custom_presets.retain(|preset| preset.name != name);
        if updated.custom_presets.len() == before {
            return None;
        }
        self.apply(settings, updated)
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

    pub(in crate::app) fn complete_save(&mut self, request: EqualizerSaveRequest) -> bool {
        if self.pending_save != Some(request) {
            return false;
        }
        self.pending_save = None;
        true
    }
}

#[cfg(test)]
mod tests;
