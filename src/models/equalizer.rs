//! Equalizer settings, shared gain bounds, and preset definitions.

use rust_i18n::t;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomEqPreset {
    pub name: String,
    #[serde(default)]
    pub preamp_db: f32,
    #[serde(default)]
    pub gains_db: [f32; 10],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EqSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub preamp_db: f32,
    #[serde(default)]
    pub gains_db: [f32; 10],
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_presets: Vec<CustomEqPreset>,
}

impl Default for EqSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            preamp_db: 0.0,
            gains_db: [0.0; 10],
            custom_presets: Vec::new(),
        }
    }
}

impl EqSettings {
    pub const MIN_GAIN_DB: f32 = -12.0;
    pub const MAX_GAIN_DB: f32 = 12.0;

    pub const BANDS: [f32; 10] = [
        31.25, 62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
    ];

    pub const PRESETS: [(&'static str, [f32; 10]); 8] = [
        ("Flat", [0.0; 10]),
        (
            "Bass boost",
            [7.0, 6.0, 4.5, 3.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ),
        (
            "Treble boost",
            [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.5, 5.0, 6.5],
        ),
        (
            "Vocal",
            [-2.0, -1.5, 0.0, 2.0, 4.0, 4.0, 3.0, 1.5, 0.0, -1.0],
        ),
        ("Rock", [5.0, 4.0, 3.0, 1.0, -0.5, -1.0, 0.5, 2.5, 3.5, 4.0]),
        ("Pop", [-1.0, 0.5, 3.0, 4.0, 3.0, 0.5, -1.0, -1.0, 0.0, 1.0]),
        ("Jazz", [4.0, 3.0, 1.0, 2.0, -1.0, -1.0, 0.0, 1.0, 2.5, 3.5]),
        (
            "Classical",
            [4.0, 3.0, 2.0, 0.0, -1.0, -1.0, 0.0, 2.5, 3.5, 4.5],
        ),
    ];

    pub fn preset_label(name: &'static str) -> std::borrow::Cow<'static, str> {
        match name {
            "Flat" => t!("Flat"),
            "Bass boost" => t!("Bass boost"),
            "Treble boost" => t!("Treble boost"),
            "Vocal" => t!("Vocal"),
            "Rock" => t!("Rock"),
            "Pop" => t!("Pop"),
            "Jazz" => t!("Jazz"),
            "Classical" => t!("Classical"),
            other => std::borrow::Cow::Borrowed(other),
        }
    }

    pub fn clamp_gains(&mut self) {
        self.preamp_db = Self::normalize_gain(self.preamp_db);
        for gain in &mut self.gains_db {
            *gain = Self::normalize_gain(*gain);
        }
        for preset in &mut self.custom_presets {
            preset.preamp_db = Self::normalize_gain(preset.preamp_db);
            for gain in &mut preset.gains_db {
                *gain = Self::normalize_gain(*gain);
            }
        }
    }

    /// Invalid gains fall back to neutral; finite gains stay within the UI range.
    pub fn normalize_gain(gain_db: f32) -> f32 {
        if gain_db.is_finite() {
            gain_db.clamp(Self::MIN_GAIN_DB, Self::MAX_GAIN_DB)
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests;
