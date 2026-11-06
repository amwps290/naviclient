//! Ordered detail batches and missing-track reconciliation, without network I/O.

use std::collections::HashMap;

use serde_json::Value;

use super::response::value_as_u64;

pub(super) const SONG_BATCH_SIZE: usize = 500;

pub(super) struct TrackHydration {
    track_ids: Vec<String>,
    next: usize,
    songs_by_id: HashMap<String, Value>,
}

impl TrackHydration {
    /// Embedded tracks are retained when their count already covers the IDs.
    /// Otherwise refetch all IDs; embedded data does not seed detail results.
    pub(super) fn new(playlist: &Value) -> Option<Self> {
        let track_ids: Vec<String> = playlist
            .get("trackIds")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|track| track.get("id").and_then(value_as_u64))
            .map(|id| id.to_string())
            .collect();
        let embedded_count = playlist
            .get("tracks")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        if track_ids.is_empty() || embedded_count >= track_ids.len() {
            return None;
        }
        Some(Self {
            songs_by_id: HashMap::with_capacity(track_ids.len()),
            track_ids,
            next: 0,
        })
    }

    pub(super) fn next_batch(&mut self) -> Option<&[String]> {
        if self.next >= self.track_ids.len() {
            return None;
        }
        let start = self.next;
        self.next = start
            .saturating_add(SONG_BATCH_SIZE)
            .min(self.track_ids.len());
        Some(&self.track_ids[start..self.next])
    }

    pub(super) fn include(&mut self, songs: Vec<Value>) {
        for song in songs {
            if let Some(id) = song.get("id").and_then(value_as_u64) {
                self.songs_by_id.insert(id.to_string(), song);
            }
        }
    }

    pub(super) fn missing_ids(&self) -> Vec<String> {
        self.track_ids
            .iter()
            .filter(|id| !self.songs_by_id.contains_key(*id))
            .cloned()
            .collect()
    }

    /// Restore playlist order and exclude unrelated detail rows. Taking each
    /// result once retains the existing unavailable count for repeated IDs.
    pub(super) fn finish(mut self) -> (Vec<Value>, usize) {
        let tracks = self
            .track_ids
            .iter()
            .filter_map(|id| self.songs_by_id.remove(id))
            .collect::<Vec<_>>();
        let unavailable = self.track_ids.len().saturating_sub(tracks.len());
        (tracks, unavailable)
    }
}

#[cfg(test)]
mod tests;
