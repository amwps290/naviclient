//! Daily Mix cache format, local day keys and versioned generation seeds.

use super::super::RecommendedTrack;
use crate::models::Song;
use chrono::Local;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

pub const DAILY_MIX_VERSION: u32 = 2;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DailyMixSnapshot {
    pub server_id: String,
    pub day: String,
    pub algorithm_version: u32,
    pub variant: u32,
    pub generated_at: u64,
    pub tracks: Vec<RecommendedTrack>,
}

impl DailyMixSnapshot {
    pub fn is_current(&self, server_id: &str, day: &str) -> bool {
        self.server_id == server_id
            && self.day == day
            && self.algorithm_version == DAILY_MIX_VERSION
    }

    pub fn songs(&self) -> Vec<Song> {
        self.tracks.iter().map(|track| track.song.clone()).collect()
    }
}

pub fn current_day_key() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

pub(super) fn daily_seed(server_id: &str, day: &str, version: u32, variant: u32) -> u64 {
    let mut hasher = Md5::new();
    hasher.update(server_id.as_bytes());
    hasher.update(day.as_bytes());
    hasher.update(version.to_le_bytes());
    hasher.update(variant.to_le_bytes());
    let digest = hasher.finalize();
    u64::from_le_bytes([
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
    ])
}
