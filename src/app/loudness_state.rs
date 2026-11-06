//! Server-scoped measured loudness and selection of playback normalization gain.

use std::collections::HashMap;

use crate::models::{Song, VolumeNormalization};

#[derive(Default)]
pub(super) struct LoudnessCache {
    server_id: Option<String>,
    measurements: HashMap<String, (f32, f32)>,
}

impl LoudnessCache {
    pub(super) fn server_id(&self) -> Option<&str> {
        self.server_id.as_deref()
    }

    pub(super) fn items(&self) -> &HashMap<String, (f32, f32)> {
        &self.measurements
    }

    pub(super) fn measurement(&self, song_id: &str) -> Option<(f32, f32)> {
        self.measurements.get(song_id).copied()
    }

    pub(super) fn restore(
        &mut self,
        server_id: Option<&str>,
        mut measurements: HashMap<String, (f32, f32)>,
    ) {
        measurements.retain(|song_id, (rms_db, peak)| valid_measurement(song_id, *rms_db, *peak));
        self.server_id = server_id.map(str::to_string);
        self.measurements = measurements;
    }

    pub(super) fn record(
        &mut self,
        mode: VolumeNormalization,
        song_id: &str,
        rms_db: f32,
        peak: f32,
    ) -> bool {
        if mode == VolumeNormalization::Off || !valid_measurement(song_id, rms_db, peak) {
            return false;
        }
        self.measurements
            .insert(song_id.to_string(), (rms_db, peak));
        true
    }

    /// Prefer the selected ReplayGain tag, then a local measurement, then unity.
    pub(super) fn gain(&self, song: &Song, mode: VolumeNormalization) -> f32 {
        let server_gain = match mode {
            VolumeNormalization::Off => return 1.0,
            VolumeNormalization::Track => song.replay_gain_track_gain,
            VolumeNormalization::Album => song.replay_gain_album_gain,
        };
        if server_gain.is_some() {
            return replay_gain_factor(
                mode,
                song.replay_gain_track_gain,
                song.replay_gain_track_peak,
                song.replay_gain_album_gain,
                song.replay_gain_album_peak,
            );
        }
        self.measurement(&song.id)
            .map_or(1.0, |(rms_db, peak)| loudness_gain_factor(rms_db, peak))
    }
}

fn valid_measurement(song_id: &str, rms_db: f32, peak: f32) -> bool {
    !song_id.trim().is_empty()
        && rms_db != 0.0
        && rms_db.is_finite()
        && peak.is_finite()
        && peak >= 0.0
}

/// 根据 ReplayGain 元数据与标准化模式计算增益系数（1.0 = 不变）。
/// 正增益（dB）表示歌曲偏轻需放大，负增益表示偏响需降低；结合峰值做防削波。
fn replay_gain_factor(
    mode: VolumeNormalization,
    track_gain_db: Option<f32>,
    track_peak: Option<f32>,
    album_gain_db: Option<f32>,
    album_peak: Option<f32>,
) -> f32 {
    let (gain_db, peak) = match mode {
        VolumeNormalization::Off => return 1.0,
        VolumeNormalization::Track => (track_gain_db, track_peak),
        VolumeNormalization::Album => (album_gain_db, album_peak),
    };
    let Some(gain_db) = gain_db.filter(|gain| gain.is_finite()) else {
        return 1.0;
    };
    let mut factor = 10f32.powf(gain_db / 20.0);
    // 防削波：有峰值元数据时，增益不能把输出峰值推过 1.0。
    if let Some(peak) = peak.filter(|peak| peak.is_finite() && *peak > 0.0) {
        factor = factor.min(1.0 / peak);
    }
    factor.clamp(0.1, 4.0)
}

/// 本地响度标准化的目标 RMS（dBFS）。
const LOUDNESS_TARGET_DB: f32 = -16.0;

/// 依据本地测量的响度与峰值计算增益系数（1.0 = 不变）。
fn loudness_gain_factor(measured_rms_db: f32, peak: f32) -> f32 {
    let gain_db = LOUDNESS_TARGET_DB - measured_rms_db;
    let mut factor = 10f32.powf(gain_db / 20.0);
    if peak > 0.0 && peak.is_finite() {
        factor = factor.min(1.0 / peak);
    }
    factor.clamp(0.1, 4.0)
}

#[cfg(test)]
mod tests;
