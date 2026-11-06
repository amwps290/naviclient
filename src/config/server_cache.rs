//! Server-scoped JSON data and legacy adoption after successful persistence.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{de::DeserializeOwned, Serialize};

use super::json::{read_json_at, write_json_atomic};
use super::migration::migrate_legacy_playback_mode;
use super::paths::{config_dir, server_scope};
use crate::models::{Genre, ListeningHistory, PlaybackSession};
use crate::recommendation::DailyMixSnapshot;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum CacheKind {
    Session,
    Loudness,
    Genres,
    ListeningHistory,
    DailyMix,
}

impl CacheKind {
    const ALL: [Self; 5] = [
        Self::Session,
        Self::Loudness,
        Self::Genres,
        Self::ListeningHistory,
        Self::DailyMix,
    ];

    fn stem(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Loudness => "loudness",
            Self::Genres => "genres",
            Self::ListeningHistory => "listening-history",
            Self::DailyMix => "daily-mix",
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Loudness => "loudness cache",
            Self::Genres => "genres cache",
            Self::ListeningHistory => "listening history",
            Self::DailyMix => "daily mix",
        }
    }
}

/// Typed snapshots retain each cache's existing JSON shape. Serialization is
/// performed by the background saver rather than while submitting on the UI.
#[derive(Clone)]
pub(crate) enum CacheSnapshot {
    Session(PlaybackSession),
    Loudness(HashMap<String, (f32, f32)>),
    Genres(Vec<Genre>),
    ListeningHistory(ListeningHistory),
    DailyMix(DailyMixSnapshot),
}

impl CacheSnapshot {
    pub(super) fn kind(&self) -> CacheKind {
        match self {
            Self::Session(_) => CacheKind::Session,
            Self::Loudness(_) => CacheKind::Loudness,
            Self::Genres(_) => CacheKind::Genres,
            Self::ListeningHistory(_) => CacheKind::ListeningHistory,
            Self::DailyMix(_) => CacheKind::DailyMix,
        }
    }
}

impl From<PlaybackSession> for CacheSnapshot {
    fn from(session: PlaybackSession) -> Self {
        Self::Session(session)
    }
}

struct ServerCache {
    dir: PathBuf,
    scope: String,
}

impl ServerCache {
    fn new(dir: PathBuf, server_id: Option<&str>) -> Self {
        Self {
            dir,
            scope: server_scope(server_id),
        }
    }

    fn current(server_id: Option<&str>) -> Self {
        Self::new(config_dir(), server_id)
    }

    fn path(&self, kind: CacheKind) -> PathBuf {
        self.dir
            .join(format!("{}-{}.json", kind.stem(), self.scope))
    }

    fn legacy_path(&self, kind: CacheKind) -> PathBuf {
        self.dir.join(format!("{}.json", kind.stem()))
    }

    fn load<T: DeserializeOwned>(&self, kind: CacheKind) -> Option<T> {
        read_json_at(&self.path(kind))
    }

    fn save<T: Serialize + ?Sized>(&self, kind: CacheKind, value: &T) -> Result<()> {
        write_json_atomic(&self.path(kind), value, kind.label())
    }

    fn save_snapshot(&self, snapshot: &CacheSnapshot) -> Result<()> {
        let kind = snapshot.kind();
        match snapshot {
            CacheSnapshot::Session(value) => self.save(kind, value),
            CacheSnapshot::Loudness(value) => self.save(kind, value),
            CacheSnapshot::Genres(value) => self.save(kind, value),
            CacheSnapshot::ListeningHistory(value) => self.save(kind, value),
            CacheSnapshot::DailyMix(value) => self.save(kind, value),
        }
    }

    /// The scoped copy takes precedence. A failed write leaves the legacy source
    /// intact and still returns its data so playback can continue in memory.
    fn load_or_migrate<T: Serialize>(
        &self,
        kind: CacheKind,
        read: impl Fn(&Path) -> Option<T>,
        accepts_legacy: impl Fn(&T) -> bool,
    ) -> Option<T> {
        if let Some(value) = read(&self.path(kind)) {
            return Some(value);
        }
        let legacy = self.legacy_path(kind);
        let value = read(&legacy)?;
        if !accepts_legacy(&value) {
            return None;
        }
        match self.save(kind, &value) {
            Ok(()) => match fs::remove_file(&legacy) {
                Ok(()) => log::info!(
                    "migrated legacy {} into server scope {}",
                    kind.label(),
                    self.scope
                ),
                Err(error) => log::warn!(
                    "migrated {} but could not remove its legacy source {}: {error}",
                    kind.label(),
                    legacy.display()
                ),
            },
            Err(error) => log::warn!("failed to migrate legacy {}: {error:#}", kind.label()),
        }
        Some(value)
    }

    fn loudness(&self) -> HashMap<String, (f32, f32)> {
        // Legacy measurements and genres have no server identity; the first
        // active server that persists them adopts the existing global file.
        self.load_or_migrate(CacheKind::Loudness, read_json_at, |_| true)
            .unwrap_or_default()
    }

    fn genres(&self) -> Option<Vec<Genre>> {
        self.load_or_migrate(CacheKind::Genres, read_json_at, |_| true)
    }

    fn session(&self, server_url: &str) -> Option<PlaybackSession> {
        self.load_or_migrate(CacheKind::Session, read_session_at, |session| {
            session.matches_server(server_url)
        })
    }

    fn remove(&self) -> usize {
        CacheKind::ALL
            .into_iter()
            .filter(|kind| fs::remove_file(self.path(*kind)).is_ok())
            .count()
    }
}

fn read_session_at(path: &Path) -> Option<PlaybackSession> {
    let text = fs::read_to_string(path).ok()?;
    let mut value: serde_json::Value = serde_json::from_str(&text).ok()?;
    migrate_legacy_playback_mode(&mut value);
    serde_json::from_value(value).ok()
}

pub fn load_loudness(server_id: Option<&str>) -> HashMap<String, (f32, f32)> {
    ServerCache::current(server_id).loudness()
}

pub fn load_genres_cache(server_id: Option<&str>) -> Option<Vec<Genre>> {
    ServerCache::current(server_id).genres()
}

pub fn load_session(server_id: Option<&str>, server_url: &str) -> Option<PlaybackSession> {
    ServerCache::current(server_id).session(server_url)
}

pub fn load_listening_history(server_id: Option<&str>) -> ListeningHistory {
    ServerCache::current(server_id)
        .load(CacheKind::ListeningHistory)
        .unwrap_or_default()
}

pub fn load_daily_mix(server_id: Option<&str>) -> Option<DailyMixSnapshot> {
    ServerCache::current(server_id).load(CacheKind::DailyMix)
}

pub(super) fn save_snapshot(server_id: Option<&str>, snapshot: &CacheSnapshot) -> Result<()> {
    ServerCache::current(server_id).save_snapshot(snapshot)
}

/// Cover and audio files retain their content hashes and separate eviction policy.
pub fn remove_server_cache(server_id: &str) -> usize {
    ServerCache::current(Some(server_id)).remove()
}

#[cfg(test)]
mod tests;
