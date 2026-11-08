use serde::{Deserialize, Serialize};

use super::source::{ProviderKind, SourceId};
use super::{PlayModes, Song};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueOrigin {
    #[default]
    Manual,
    Playlist,
    DailyMix,
    SongRadio,
    ArtistRadio,
    AutoContinue,
}

fn normalize_queue_origins(mut origins: Vec<QueueOrigin>, queue_len: usize) -> Vec<QueueOrigin> {
    origins.resize(queue_len, QueueOrigin::Manual);
    origins.truncate(queue_len);
    origins
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueEntryId(u64);

/// Moving an entry retains its identity; copying it creates another occurrence.
#[derive(Debug)]
pub struct QueueEntry {
    id: QueueEntryId,
    pub song: Song,
    pub origin: QueueOrigin,
}

impl QueueEntry {
    pub fn new(song: Song, origin: QueueOrigin) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            id: QueueEntryId(NEXT.fetch_add(1, Ordering::Relaxed)),
            song,
            origin,
        }
    }

    pub fn id(&self) -> QueueEntryId {
        self.id
    }
}

impl Clone for QueueEntry {
    fn clone(&self) -> Self {
        Self::new(self.song.clone(), self.origin)
    }
}

/// Runtime queue storage. Mutations always move a complete occurrence.
/// The legacy parallel arrays exist only at the session persistence boundary.
#[derive(Debug, Default)]
pub struct PlaybackQueue {
    entries: Vec<QueueEntry>,
}

impl PlaybackQueue {
    /// Bind legacy entries before accepting a queue for one source. Validation
    /// runs first, so a mixed queue is rejected without partially changing it.
    pub fn bind_source(&mut self, source: &SourceId) -> bool {
        if self.songs().any(|song| !song.belongs_to(source)) {
            return false;
        }
        for entry in &mut self.entries {
            entry.song.source = Some(source.clone());
        }
        true
    }
    pub fn from_songs(songs: Vec<Song>, origin: QueueOrigin) -> Self {
        Self {
            entries: songs
                .into_iter()
                .map(|song| QueueEntry::new(song, origin))
                .collect(),
        }
    }

    pub fn from_snapshot(songs: Vec<Song>, origins: Vec<QueueOrigin>) -> Self {
        let origins = normalize_queue_origins(origins, songs.len());
        Self {
            entries: songs
                .into_iter()
                .zip(origins)
                .map(|(song, origin)| QueueEntry::new(song, origin))
                .collect(),
        }
    }

    pub fn entries(&self) -> &[QueueEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn song(&self, index: usize) -> Option<&Song> {
        self.entries.get(index).map(|entry| &entry.song)
    }

    pub fn entry_id(&self, index: usize) -> Option<QueueEntryId> {
        self.entries.get(index).map(QueueEntry::id)
    }

    pub fn index_of(&self, id: QueueEntryId) -> Option<usize> {
        self.entries.iter().position(|entry| entry.id() == id)
    }

    pub fn origin(&self, index: usize) -> Option<QueueOrigin> {
        self.entries.get(index).map(|entry| entry.origin)
    }

    pub fn songs(&self) -> impl DoubleEndedIterator<Item = &Song> + ExactSizeIterator {
        self.entries.iter().map(|entry| &entry.song)
    }

    pub fn song_ids(&self) -> Vec<String> {
        self.songs().map(|song| song.id.clone()).collect()
    }

    pub fn snapshot(&self) -> (Vec<Song>, Vec<QueueOrigin>) {
        self.entries
            .iter()
            .map(|entry| (entry.song.clone(), entry.origin))
            .unzip()
    }

    /// Consume prepared songs, creating new occurrences without cloning metadata.
    pub fn insert(&mut self, index: usize, songs: Vec<Song>, origin: QueueOrigin) {
        let index = index.min(self.len());
        self.entries.splice(
            index..index,
            songs.into_iter().map(|song| QueueEntry::new(song, origin)),
        );
    }

    pub fn append_entries(&mut self, entries: impl IntoIterator<Item = QueueEntry>) {
        self.entries.extend(entries);
    }

    pub fn remove(&mut self, index: usize) -> Option<QueueEntry> {
        (index < self.len()).then(|| self.entries.remove(index))
    }

    pub fn truncate(&mut self, len: usize) {
        self.entries.truncate(len);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn move_entry(&mut self, from: usize, to: usize) -> bool {
        if from >= self.len() || to >= self.len() || from == to {
            return false;
        }
        let entry = self.entries.remove(from);
        self.entries.insert(to, entry);
        true
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PlaybackSession {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceId>,
    /// 归属服务器（切换服务器时不恢复其他服务器的会话）。
    #[serde(default)]
    pub server_url: String,
    /// 播放队列快照。
    pub queue: Vec<Song>,
    /// 与 queue 按索引对齐的来源；旧会话缺失时迁移为 Manual。
    #[serde(default)]
    pub queue_origins: Vec<QueueOrigin>,
    /// 当前播放的队列索引。
    pub queue_index: Option<usize>,
    /// 播放位置（秒）。
    pub position_secs: f64,
    /// 播放模式（随机开关 + 循环模式）。
    pub playback_modes: PlayModes,
    /// 随机播放历史（播放过的索引）。
    pub shuffle_played: Vec<usize>,
    /// 随机播放回退暂存。
    pub shuffle_forward: Vec<usize>,
}

impl PlaybackSession {
    pub(crate) fn matches_source(&self, source: &SourceId, server_url: Option<&str>) -> bool {
        if self.source.as_ref().is_some_and(|saved| saved != source) {
            return false;
        }
        match source.provider {
            ProviderKind::OpenSubsonic => server_url.is_some_and(|url| self.matches_server(url)),
            ProviderKind::Bilibili => self.source.as_ref() == Some(source),
        }
    }
    /// Session loading and legacy adoption share the same server identity.
    /// Request construction accepts addresses without a scheme and strips a
    /// trailing slash, so their saved forms must still identify the same server.
    pub(crate) fn matches_server(&self, server_url: &str) -> bool {
        fn address(raw: &str) -> Option<url::Url> {
            let raw = raw.trim().trim_end_matches('/');
            if raw.is_empty() {
                return None;
            }
            let mut parsed = if raw.contains("://") {
                url::Url::parse(raw).ok()?
            } else {
                url::Url::parse(&format!("http://{raw}")).ok()?
            };
            if !matches!(parsed.scheme(), "http" | "https") {
                return None;
            }
            // HTTP hosts and default ports are normalized by Url. Paths remain
            // case-sensitive, including distinct reverse-proxy mounts.
            let path = parsed.path().trim_end_matches('/').to_string();
            parsed.set_path(&path);
            Some(parsed)
        }
        address(&self.server_url)
            .zip(address(server_url))
            .is_some_and(|(saved, current)| saved == current)
    }
}

#[cfg(test)]
mod tests;
