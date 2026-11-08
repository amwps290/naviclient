use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::source::MediaKey;
use super::Song;

pub const LISTENING_HISTORY_LIMIT: usize = 1_000;

/// One local listening attempt. History is scoped by server in `config.rs` and
/// stays independent from Subsonic scrobble state: short previews are useful
/// for suppression, while `completed` attempts contribute stronger affinity.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListeningEvent {
    pub song: Song,
    pub started_at: u64,
    #[serde(default)]
    pub listened_seconds: u32,
    #[serde(default)]
    pub completed: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ListeningHistory {
    #[serde(default)]
    pub events: Vec<ListeningEvent>,
}

impl ListeningHistory {
    pub fn update_latest_media(
        &mut self,
        media: &MediaKey,
        listened_seconds: u32,
        completed: bool,
    ) -> bool {
        let Some(event) = self.events.iter_mut().rev().find(|event| {
            event.song.source.as_ref() == Some(&media.source) && event.song.id == media.remote_id
        }) else {
            return false;
        };
        event.listened_seconds = event.listened_seconds.max(listened_seconds);
        event.completed |= completed;
        true
    }
    pub fn record_started(&mut self, song: Song, started_at: u64) {
        if song.id.trim().is_empty() {
            return;
        }
        self.events.push(ListeningEvent {
            song,
            started_at,
            listened_seconds: 0,
            completed: false,
        });
        let excess = self.events.len().saturating_sub(LISTENING_HISTORY_LIMIT);
        if excess > 0 {
            self.events.drain(..excess);
        }
    }

    /// Update the newest attempt for a song. Returning false means no start
    /// event exists, which lets the caller recover without inventing history.
    pub fn update_latest(&mut self, song_id: &str, listened_seconds: u32, completed: bool) -> bool {
        let Some(event) = self
            .events
            .iter_mut()
            .rev()
            .find(|event| event.song.id == song_id)
        else {
            return false;
        };
        event.listened_seconds = event.listened_seconds.max(listened_seconds);
        event.completed |= completed;
        true
    }

    pub fn recent_songs(&self, limit: usize) -> Vec<Song> {
        self.events
            .iter()
            .rev()
            .take(limit)
            .map(|event| event.song.clone())
            .collect()
    }

    pub fn recent_song_ids(&self, limit: usize) -> HashSet<String> {
        let mut ids = HashSet::with_capacity(limit.min(self.events.len()));
        if limit == 0 {
            return ids;
        }
        for event in self.events.iter().rev() {
            if !event.song.id.trim().is_empty() {
                ids.insert(event.song.id.clone());
            }
            if ids.len() == limit {
                break;
            }
        }
        ids
    }
}
