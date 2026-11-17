//! Lyrics providers are independent of audio sources and their credentials.

pub(crate) mod cache;
pub(crate) mod kugou;
pub(crate) mod lrc;
pub(crate) mod lrclib;
pub(crate) mod matching;
pub(crate) mod netease;
pub(crate) mod online;
pub(crate) mod provider;
pub(crate) mod service;
pub(crate) mod timing;
mod transport;
pub(crate) mod yrc;

use crate::models::{network::SourceConnection, Lyrics};
use provider::{ProviderId, Query, Record};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LyricsPolicy {
    #[default]
    SourceFirst,
    OnlineFirst,
    Manual,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct LyricsSettings {
    pub enabled: bool,
    pub policy: LyricsPolicy,
    pub connection: SourceConnection,
    pub sources: Vec<ProviderId>,
    pub word_timing: bool,
}

impl Default for LyricsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            policy: LyricsPolicy::default(),
            connection: SourceConnection::default(),
            // Upgrading preserves the set of services previously enabled.
            sources: vec![ProviderId::Lrclib],
            word_timing: true,
        }
    }
}

impl LyricsSettings {
    pub fn enabled_sources(&self) -> Vec<ProviderId> {
        ProviderId::ALL
            .into_iter()
            .filter(|id| self.sources.contains(id))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LyricsOrigin {
    #[default]
    Native,
    Lrclib,
    Kugou,
    Netease,
}

#[derive(Clone, Debug)]
pub(crate) struct Document {
    pub lyrics: Lyrics,
    pub origin: LyricsOrigin,
    pub record: Option<Record>,
    pub instrumental: bool,
    pub manual: bool,
    pub temporary: bool,
    pub plain: bool,
    pub offset_ms: i64,
    pub timing_edits: Option<timing::TimingEdits>,
    pub query: Option<Query>,
}

impl Document {
    pub fn native(lyrics: Lyrics) -> Self {
        Self {
            lyrics,
            origin: LyricsOrigin::Native,
            record: None,
            instrumental: false,
            manual: false,
            temporary: false,
            plain: false,
            offset_ms: 0,
            timing_edits: None,
            query: None,
        }
    }

    pub fn online(record: Record) -> Self {
        let mut document = Self::native(record.lyrics());
        document.origin = match record.source {
            ProviderId::Lrclib => LyricsOrigin::Lrclib,
            ProviderId::Kugou => LyricsOrigin::Kugou,
            ProviderId::Netease => LyricsOrigin::Netease,
        };
        document.instrumental = record.instrumental;
        document.record = Some(record);
        document
    }

    pub fn display_lyrics(&self) -> Lyrics {
        let mut lyrics = self.edited_lyrics();
        for line in &mut lyrics.lines {
            line.end_ms = if self.plain {
                None
            } else {
                line.end_ms.map(|ms| adjusted_time(ms, self.offset_ms))
            };
            if self.plain {
                line.word_timing = None;
            } else if let Some(words) = &mut line.word_timing {
                words.start_ms = adjusted_time(words.start_ms, self.offset_ms);
                words.end_ms = words.end_ms.map(|ms| adjusted_time(ms, self.offset_ms));
                for segment in &mut words.segments {
                    segment.start_ms = adjusted_time(segment.start_ms, self.offset_ms);
                    segment.end_ms = segment.end_ms.map(|ms| adjusted_time(ms, self.offset_ms));
                }
            }
            line.start_ms = if self.plain {
                None
            } else {
                line.start_ms.map(|ms| adjusted_time(ms, self.offset_ms))
            };
        }
        lyrics
    }
}

pub(crate) fn adjusted_time(ms: u64, offset: i64) -> u64 {
    (i128::from(ms) + i128::from(offset)).clamp(0, i128::from(u64::MAX)) as u64
}
