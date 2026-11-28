//! Append-only JSONL persistence for qualified local listening events.

use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::models::{ListeningEvent, Song};
use crate::source::SourceId;

const MIN_QUALIFIED_SECONDS: u32 = 30;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListeningLogEntry {
    pub ts: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceId>,
    pub song_id: String,
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<i32>,
    pub listened_secs: u32,
    #[serde(default)]
    pub completed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub song: Option<Song>,
}

impl ListeningLogEntry {
    pub fn from_event(event: &ListeningEvent) -> Self {
        Self {
            ts: event.started_at,
            source: event.song.source.clone(),
            song_id: event.song.id.clone(),
            title: event.song.title.clone(),
            artist: event.song.artist.clone(),
            album: event.song.album.clone(),
            genre: event.song.genre.clone(),
            duration_secs: event.song.duration,
            listened_secs: event.listened_seconds,
            completed: event.completed,
            song: Some(event.song.clone()),
        }
    }

    pub fn to_song(&self) -> Song {
        if let Some(song) = &self.song {
            return song.clone();
        }
        Song {
            source: self.source.clone(),
            id: self.song_id.clone(),
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            genre: self.genre.clone(),
            duration: self.duration_secs,
            ..Default::default()
        }
    }
}

/// A play counts toward insights when it finishes naturally, reaches 30 seconds,
/// or covers at least half of a short track's known duration.
pub fn is_qualified_play(listened_secs: u32, duration_secs: Option<i32>, completed: bool) -> bool {
    if listened_secs == 0 && !completed {
        return false;
    }
    if completed || listened_secs >= MIN_QUALIFIED_SECONDS {
        return true;
    }
    if let Some(dur) = duration_secs.filter(|&d| d > 0) {
        let half = (dur as u32).div_ceil(2);
        return listened_secs >= half.max(5);
    }
    false
}

pub struct ListeningLog {
    path: PathBuf,
    entries: Vec<ListeningLogEntry>,
    seen: HashSet<(u64, String)>,
}

impl ListeningLog {
    pub fn open(path: PathBuf) -> Self {
        let mut log = Self {
            path,
            entries: Vec::new(),
            seen: HashSet::new(),
        };
        log.reload();
        log
    }

    pub fn reload(&mut self) {
        self.entries.clear();
        self.seen.clear();
        let Ok(file) = fs::File::open(&self.path) else {
            return;
        };
        let reader = BufReader::new(file);
        for line in reader.lines().map_while(Result::ok) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<ListeningLogEntry>(trimmed) {
                let key = (entry.ts, entry.song_id.clone());
                if self.seen.insert(key) {
                    self.entries.push(entry);
                }
            }
        }
    }

    #[cfg(test)]
    pub fn entries(&self) -> &[ListeningLogEntry] {
        &self.entries
    }

    pub fn record_event(&mut self, event: &ListeningEvent) -> Result<bool> {
        if event.song.id.trim().is_empty()
            || !is_qualified_play(event.listened_seconds, event.song.duration, event.completed)
        {
            return Ok(false);
        }
        let key = (event.started_at, event.song.id.clone());
        if !self.seen.insert(key) {
            return Ok(false);
        }
        let entry = ListeningLogEntry::from_event(event);
        append_jsonl(&self.path, &entry)?;
        self.entries.push(entry);
        Ok(true)
    }

    pub fn export_all(&self) -> Vec<ListeningLogEntry> {
        self.entries.clone()
    }

    pub fn import_entries(
        &mut self,
        incoming: Vec<ListeningLogEntry>,
        overwrite: bool,
    ) -> Result<usize> {
        if overwrite {
            self.entries.clear();
            self.seen.clear();
            let mut count = 0usize;
            for entry in incoming {
                if entry.song_id.trim().is_empty() {
                    continue;
                }
                let key = (entry.ts, entry.song_id.clone());
                if self.seen.insert(key) {
                    self.entries.push(entry);
                    count += 1;
                }
            }
            rewrite_jsonl(&self.path, &self.entries)?;
            return Ok(count);
        }
        let mut added = 0usize;
        for entry in incoming {
            if entry.song_id.trim().is_empty() {
                continue;
            }
            let key = (entry.ts, entry.song_id.clone());
            if self.seen.insert(key) {
                append_jsonl(&self.path, &entry)?;
                self.entries.push(entry);
                added += 1;
            }
        }
        Ok(added)
    }

    /// Merge historical `ListeningEvent`s from server-scoped history into a combined slice
    /// without duplicating entries already persisted in `listening-log.jsonl`.
    pub fn merged_entries(&self, extra_events: &[ListeningEvent]) -> Vec<ListeningLogEntry> {
        let mut combined = self.entries.clone();
        let mut seen = self.seen.clone();
        for event in extra_events {
            if event.song.id.trim().is_empty()
                || !is_qualified_play(event.listened_seconds, event.song.duration, event.completed)
            {
                continue;
            }
            let key = (event.started_at, event.song.id.clone());
            if seen.insert(key) {
                combined.push(ListeningLogEntry::from_event(event));
            }
        }
        combined
    }
}

fn append_jsonl(path: &Path, entry: &ListeningLogEntry) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
    }
    let mut line = serde_json::to_string(entry).context("failed to serialize listening entry")?;
    line.push('\n');
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    file.write_all(line.as_bytes())
        .with_context(|| format!("failed to append {}", path.display()))?;
    Ok(())
}

fn rewrite_jsonl(path: &Path, entries: &[ListeningLogEntry]) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
    }
    let mut buf = String::new();
    for entry in entries {
        let line = serde_json::to_string(entry).context("failed to serialize listening entry")?;
        buf.push_str(&line);
        buf.push('\n');
    }
    let tmp = path.with_extension("jsonl.tmp");
    fs::write(&tmp, buf.as_bytes()).with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}
