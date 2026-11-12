//! Versioned local content, query index and source-scoped personal preferences.

use super::{
    matching,
    provider::{ProviderId, Query, Record},
    Document,
};
use crate::source::MediaKey;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

const VERSION: u32 = 1;
const SUCCESS_TTL: u64 = 7 * 86400;
const MISS_TTL: u64 = 6 * 3600;
const MAX_BYTES: usize = 50 * 1024 * 1024;

mod writer;
use writer::Writer;

#[derive(Clone, Default, Serialize, Deserialize)]
struct Store {
    version: u32,
    bodies: HashMap<String, Arc<Record>>,
    queries: HashMap<String, Automatic>,
    songs: HashMap<String, Preference>,
    #[serde(skip)]
    body_bytes: HashMap<String, usize>,
    #[serde(skip)]
    bytes: usize,
}

impl Store {
    fn measure(&mut self) {
        self.body_bytes = self
            .bodies
            .iter()
            .map(|(key, record)| {
                (
                    key.clone(),
                    serde_json::to_vec(record).map_or(0, |body| body.len()),
                )
            })
            .collect();
        self.bytes = self.body_bytes.values().sum();
    }

    fn insert_body(&mut self, key: String, record: Record, bytes: usize) {
        self.bytes -= self.body_bytes.insert(key.clone(), bytes).unwrap_or(0);
        self.bytes += bytes;
        self.bodies.insert(key, Arc::new(record));
    }

    fn remove_body(&mut self, key: &str) {
        self.bodies.remove(key);
        self.bytes -= self.body_bytes.remove(key).unwrap_or(0);
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Automatic {
    body: Option<String>,
    saved: u64,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub(crate) struct Preference {
    pub media: Option<MediaKey>,
    pub body: Option<String>,
    #[serde(default)]
    pub record_id: Option<u64>,
    #[serde(default)]
    pub provider: ProviderId,
    pub query: Option<Query>,
    pub plain: bool,
    pub offset_ms: i64,
}

#[derive(Clone)]
pub(crate) struct LyricsCache {
    writer: Arc<Writer>,
}

fn digest(value: &impl Serialize) -> String {
    format!(
        "{:x}",
        Md5::digest(serde_json::to_vec(value).expect("serializable lyrics key"))
    )
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn query_key(query: &Query) -> String {
    digest(&(
        matching::VERSION,
        matching::normalize(&query.title),
        matching::normalize(&query.artist),
        matching::normalize(&query.album),
        query.duration,
    ))
}

fn provider_query_key(provider: ProviderId, query: &Query) -> String {
    // Keep LRCLIB v1 keys readable after the multi-provider upgrade.
    if provider == ProviderId::Lrclib {
        query_key(query)
    } else {
        digest(&(provider.key(), query_key(query)))
    }
}

impl Default for LyricsCache {
    fn default() -> Self {
        let path = directories::ProjectDirs::from("rs", "navidrome", "navidrome-client")
            .map(|d| d.data_local_dir().join("lyrics-v1.json"))
            .unwrap_or_else(|| PathBuf::from("navidrome-cache/lyrics-v1.json"));
        Self::at(path)
    }
}

impl LyricsCache {
    pub fn at(path: PathBuf) -> Self {
        let mut store = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Store>(&b).ok())
            .filter(|s| s.version == VERSION)
            .unwrap_or(Store {
                version: VERSION,
                ..Default::default()
            });
        store.measure();
        let writer = Writer::start(store, move |store| {
            crate::config::write_json_atomic(&path, store, "lyrics cache")
        })
        .expect("failed to start lyrics cache writer");
        Self {
            writer: Arc::new(writer),
        }
    }

    fn change(&self, change: impl FnOnce(&mut Store)) -> anyhow::Result<()> {
        self.writer.change(change)
    }

    /// Mutations are immediately visible in memory. Persistence is coalesced
    /// in the background; failed writes remain pending for retry.
    pub fn flush(&self) -> anyhow::Result<()> {
        self.writer.flush()
    }

    pub fn take_error(&self) -> Option<String> {
        self.writer.take_error()
    }

    pub fn preference(&self, media: &MediaKey) -> Preference {
        let key = digest(media);
        self.writer
            .read(|store| store.songs.get(&key).cloned().unwrap_or_default())
    }

    pub fn selected(&self, media: &MediaKey) -> Option<Document> {
        let key = digest(media);
        let (preference, record) = self.writer.read(|store| {
            let preference = store.songs.get(&key)?.clone();
            let record = store.bodies.get(preference.body.as_ref()?)?.clone();
            Some((preference, record))
        })?;
        if !record.usable()
            || record.source != preference.provider
            || preference.record_id.is_some_and(|id| id != record.id)
        {
            return None;
        }
        let mut document = Document::online((*record).clone());
        document.manual = true;
        document.plain = preference.plain;
        document.offset_ms = preference.offset_ms;
        document.query = preference.query.clone();
        Some(document)
    }

    /// None means stale/absent; Some(None) is a recent complete no-match query.
    pub fn automatic_from(&self, provider: ProviderId, query: &Query) -> Option<Option<Record>> {
        let key = provider_query_key(provider, query);
        let (hit, record) = self.writer.read(|store| {
            let hit = store.queries.get(&key)?.clone();
            let record = hit
                .body
                .as_ref()
                .and_then(|key| store.bodies.get(key))
                .cloned();
            Some((hit, record))
        })?;
        if now().saturating_sub(hit.saved)
            >= if hit.body.is_some() {
                SUCCESS_TTL
            } else {
                MISS_TTL
            }
        {
            return None;
        }
        match &hit.body {
            Some(_) => record
                .as_ref()
                .filter(|record| {
                    record.source == provider && matching::assess(query, record).trusted
                })
                .map(|record| (**record).clone())
                .map(Some),
            None => Some(None),
        }
    }

    pub fn save_automatic_from(
        &self,
        provider: ProviderId,
        query: &Query,
        record: Option<Record>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            record
                .as_ref()
                .is_none_or(|record| record.source == provider),
            "lyrics cache provider mismatch"
        );
        let record = record.map(|record| {
            let bytes = serde_json::to_vec(&record).map_or(0, |body| body.len());
            (digest(&(provider.key(), &record)), record, bytes)
        });
        let key = provider_query_key(provider, query);
        self.change(|store| {
            let body = record.map(|(key, record, bytes)| {
                store.insert_body(key.clone(), record, bytes);
                key
            });
            store.queries.insert(key, Automatic { body, saved: now() });
        })
    }

    pub fn remember(&self, media: &MediaKey, document: &Document) -> anyhow::Result<()> {
        let key = digest(media);
        let record = document.record.as_ref().map(|record| {
            let bytes = serde_json::to_vec(record).map_or(0, |body| body.len());
            (
                digest(&(record.source.key(), record)),
                record.clone(),
                bytes,
            )
        });
        self.change(|store| {
            let preference = store.songs.entry(key).or_default();
            preference.media = Some(media.clone());
            preference.query = document.query.clone();
            preference.plain = document.plain;
            preference.offset_ms = document.offset_ms;
            if let Some((body, record, bytes)) = record {
                preference.record_id = Some(record.id);
                preference.provider = record.source;
                preference.body = Some(body.clone());
                store.insert_body(body, record, bytes);
            }
        })
    }

    pub fn restore(&self, media: &MediaKey) -> anyhow::Result<()> {
        let key = digest(media);
        self.change(|store| {
            store.songs.remove(&key);
        })
    }

    pub fn clear_automatic(&self) -> anyhow::Result<()> {
        self.change(|s| s.queries.clear())
    }

    pub fn stats(&self) -> (usize, usize) {
        self.writer.read(|store| (store.bodies.len(), store.bytes))
    }

    #[cfg(test)]
    pub fn automatic(&self, query: &Query) -> Option<Option<Record>> {
        self.automatic_from(ProviderId::Lrclib, query)
    }

    #[cfg(test)]
    pub fn save_automatic(&self, query: &Query, record: Option<Record>) -> anyhow::Result<()> {
        self.save_automatic_from(ProviderId::Lrclib, query, record)
    }
}

fn prune(store: &mut Store) {
    let pins: HashSet<_> = store
        .songs
        .values()
        .filter_map(|p| p.body.clone())
        .collect();
    let mut automatic: Vec<_> = store
        .queries
        .iter()
        .map(|(k, v)| (k.clone(), v.saved))
        .collect();
    automatic.sort_by_key(|(_, saved)| *saved);
    // Track references and sizes once, rather than serializing every body for
    // every eviction. All pruning runs on the writer, outside the read lock.
    let mut references = HashMap::<String, usize>::new();
    for key in store
        .queries
        .values()
        .filter_map(|query| query.body.as_ref())
    {
        *references.entry(key.clone()).or_default() += 1;
    }
    let unused: Vec<_> = store
        .bodies
        .keys()
        .filter(|key| !pins.contains(*key) && !references.contains_key(*key))
        .cloned()
        .collect();
    for key in unused {
        store.remove_body(&key);
    }
    let mut bytes: usize = store
        .body_bytes
        .iter()
        .filter(|(key, _)| !pins.contains(*key))
        .map(|(_, bytes)| bytes)
        .sum();
    for (key, _) in automatic {
        if bytes <= MAX_BYTES && store.queries.len() <= 10_000 {
            break;
        }
        if let Some(body) = store.queries.remove(&key).and_then(|query| query.body) {
            let count = references.get_mut(&body).expect("counted query body");
            *count -= 1;
            if *count == 0 && !pins.contains(&body) {
                bytes -= store.body_bytes.get(&body).copied().unwrap_or(0);
                store.remove_body(&body);
            }
        }
    }
}

#[cfg(test)]
mod tests;
