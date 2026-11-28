//! Persistent storage for cross-source local playlists (`<config_dir>/local-playlists/<id>.json`).

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::models::Song;
use crate::source::SourceId;

static NEXT_ID_SEQ: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalPlaylist {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub songs: Vec<Song>,
}

impl LocalPlaylist {
    pub fn song_count(&self) -> usize {
        self.songs.len()
    }

    pub fn total_duration_secs(&self) -> Option<i32> {
        let total: i64 = self
            .songs
            .iter()
            .filter_map(|song| song.duration.filter(|d| *d > 0))
            .map(i64::from)
            .sum();
        (total > 0).then(|| total.min(i64::from(i32::MAX)) as i32)
    }

    pub fn cover_song(&self) -> Option<&Song> {
        self.songs.iter().find(|song| song.cover_art.is_some())
    }

    pub fn distinct_sources(&self) -> HashSet<SourceId> {
        self.songs
            .iter()
            .filter_map(|song| song.source.clone())
            .collect()
    }

    #[cfg(test)]
    pub fn is_cross_source(&self) -> bool {
        self.distinct_sources().len() > 1
    }
}

pub struct LocalPlaylistStore {
    dir: PathBuf,
    playlists: Vec<LocalPlaylist>,
}

impl LocalPlaylistStore {
    pub fn open(dir: PathBuf) -> Self {
        let mut store = Self {
            dir,
            playlists: Vec::new(),
        };
        store.reload();
        store
    }

    pub fn reload(&mut self) {
        self.playlists.clear();
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            match serde_json::from_str::<LocalPlaylist>(&text) {
                Ok(playlist) if !playlist.id.trim().is_empty() => {
                    self.playlists.push(playlist);
                }
                Ok(_) => {}
                Err(error) => {
                    log::warn!("skipping unreadable local playlist {}: {error}", path.display());
                }
            }
        }
        self.sort_playlists();
    }

    pub fn list(&self) -> &[LocalPlaylist] {
        &self.playlists
    }

    pub fn get(&self, id: &str) -> Option<&LocalPlaylist> {
        self.playlists.iter().find(|playlist| playlist.id == id)
    }

    pub fn create(&mut self, name: impl Into<String>, songs: Vec<Song>) -> Result<LocalPlaylist> {
        let name = normalize_name(&name.into())?;
        let now = unix_now_secs();
        let playlist = LocalPlaylist {
            id: generate_playlist_id(),
            name,
            created_at: now,
            updated_at: now,
            songs,
        };
        self.save_playlist(&playlist)?;
        self.playlists.push(playlist.clone());
        self.sort_playlists();
        Ok(playlist)
    }

    pub fn rename(&mut self, id: &str, name: impl Into<String>) -> Result<LocalPlaylist> {
        let name = normalize_name(&name.into())?;
        let idx = self
            .playlists
            .iter()
            .position(|playlist| playlist.id == id)
            .ok_or_else(|| anyhow!("local playlist not found"))?;
        let mut updated = self.playlists[idx].clone();
        updated.name = name;
        updated.updated_at = unix_now_secs();
        self.save_playlist(&updated)?;
        self.playlists[idx] = updated.clone();
        self.sort_playlists();
        Ok(updated)
    }

    pub fn delete(&mut self, id: &str) -> Result<()> {
        let idx = self
            .playlists
            .iter()
            .position(|playlist| playlist.id == id)
            .ok_or_else(|| anyhow!("local playlist not found"))?;
        let path = self.playlist_path(id);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
        self.playlists.remove(idx);
        Ok(())
    }

    pub fn append_songs(&mut self, id: &str, songs: Vec<Song>) -> Result<LocalPlaylist> {
        let idx = self
            .playlists
            .iter()
            .position(|playlist| playlist.id == id)
            .ok_or_else(|| anyhow!("local playlist not found"))?;
        let mut updated = self.playlists[idx].clone();
        updated.songs.extend(songs);
        updated.updated_at = unix_now_secs();
        self.save_playlist(&updated)?;
        self.playlists[idx] = updated.clone();
        self.sort_playlists();
        Ok(updated)
    }

    pub fn remove_song(&mut self, id: &str, index: usize) -> Result<LocalPlaylist> {
        let idx = self
            .playlists
            .iter()
            .position(|playlist| playlist.id == id)
            .ok_or_else(|| anyhow!("local playlist not found"))?;
        let mut updated = self.playlists[idx].clone();
        if index >= updated.songs.len() {
            return Err(anyhow!("track index out of range"));
        }
        updated.songs.remove(index);
        updated.updated_at = unix_now_secs();
        self.save_playlist(&updated)?;
        self.playlists[idx] = updated.clone();
        Ok(updated)
    }

    pub fn move_song(&mut self, id: &str, from: usize, to: usize) -> Result<LocalPlaylist> {
        let idx = self
            .playlists
            .iter()
            .position(|playlist| playlist.id == id)
            .ok_or_else(|| anyhow!("local playlist not found"))?;
        let mut updated = self.playlists[idx].clone();
        if from >= updated.songs.len() || to >= updated.songs.len() {
            return Err(anyhow!("track index out of range"));
        }
        if from != to {
            let song = updated.songs.remove(from);
            updated.songs.insert(to, song);
            updated.updated_at = unix_now_secs();
            self.save_playlist(&updated)?;
            self.playlists[idx] = updated.clone();
        }
        Ok(updated)
    }

    fn playlist_path(&self, id: &str) -> PathBuf {
        let safe: String = id
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
            .collect();
        self.dir.join(format!("{safe}.json"))
    }

    fn save_playlist(&self, playlist: &LocalPlaylist) -> Result<()> {
        fs::create_dir_all(&self.dir)
            .with_context(|| format!("failed to create {}", self.dir.display()))?;
        let path = self.playlist_path(&playlist.id);
        crate::config::write_json_atomic(&path, playlist, "local playlist")
    }

    fn sort_playlists(&mut self) {
        self.playlists.sort_by(|a, b| {
            b.updated_at
                .cmp(&a.updated_at)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then_with(|| a.id.cmp(&b.id))
        });
    }
}

fn normalize_name(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("playlist name cannot be empty"));
    }
    Ok(trimmed.to_string())
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn generate_playlist_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seq = NEXT_ID_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("lp-{millis}-{seq}")
}
