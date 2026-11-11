//! Lyrics providers are independent of audio sources and their credentials.

pub(crate) mod cache;
pub(crate) mod lrc;
pub(crate) mod lrclib;
pub(crate) mod matching;
pub(crate) mod provider;
pub(crate) mod service;

use crate::models::{network::SourceConnection, Lyrics};
use provider::{Query, Record};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LyricsPolicy {
    #[default]
    SourceFirst,
    OnlineFirst,
    Manual,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct LyricsSettings {
    pub enabled: bool,
    pub policy: LyricsPolicy,
    pub connection: SourceConnection,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LyricsOrigin {
    #[default]
    Native,
    Lrclib,
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
            query: None,
        }
    }

    pub fn online(record: Record) -> Self {
        let mut document = Self::native(record.lyrics());
        document.origin = LyricsOrigin::Lrclib;
        document.instrumental = record.instrumental;
        document.record = Some(record);
        document
    }

    pub fn display_lyrics(&self) -> Lyrics {
        let mut lyrics = self.lyrics.clone();
        for line in &mut lyrics.lines {
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
