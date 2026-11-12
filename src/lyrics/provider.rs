//! Stable provider DTOs; no audio addresses, account names, or credentials.

use crate::models::{Lyrics, Song};
use crate::source::ProviderKind;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderId {
    #[default]
    Lrclib,
    Kugou,
    Netease,
}

impl ProviderId {
    pub const ALL: [Self; 3] = [Self::Lrclib, Self::Kugou, Self::Netease];

    pub fn key(self) -> &'static str {
        match self {
            Self::Lrclib => "lrclib",
            Self::Kugou => "kugou",
            Self::Netease => "netease",
        }
    }

    fn is_lrclib(&self) -> bool {
        *self == Self::Lrclib
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct Query {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: Option<u32>,
}

impl Query {
    pub fn for_song(song: &Song) -> Self {
        let video = song.bilibili_part.is_some()
            || song
                .source
                .as_ref()
                .is_some_and(|s| s.provider == ProviderKind::Bilibili);
        Self {
            title: song
                .bilibili_part
                .as_ref()
                .map(|p| p.title.clone())
                .unwrap_or_else(|| song.title.clone()),
            artist: if video {
                String::new()
            } else {
                song.artist.clone()
            },
            album: if video {
                String::new()
            } else {
                song.album.clone()
            },
            duration: song
                .duration
                .and_then(|d| u32::try_from(d).ok())
                .filter(|d| (1..=3600).contains(d)),
        }
    }

    pub fn automatic(&self) -> bool {
        !self.title.trim().is_empty() && !self.artist.trim().is_empty() && self.duration.is_some()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Record {
    pub id: u64,
    #[serde(default, skip_serializing_if = "ProviderId::is_lrclib")]
    pub source: ProviderId,
    #[serde(default)]
    pub track_name: String,
    #[serde(default)]
    pub artist_name: String,
    #[serde(default)]
    pub album_name: String,
    #[serde(default)]
    pub duration: f64,
    #[serde(default)]
    pub instrumental: bool,
    pub plain_lyrics: Option<String>,
    pub synced_lyrics: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SearchOutcome {
    pub records: Vec<Record>,
    pub failures: Vec<(ProviderId, ProviderError)>,
}

impl Record {
    pub(super) fn set_text(&mut self, text: &str) {
        let lyrics = super::lrc::parse(text);
        self.plain_lyrics = Some(if lyrics.is_synced() {
            lyrics
                .lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            text.trim_start_matches('\u{feff}')
                .lines()
                .take(10_000)
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('['))
                .map(|line| line.chars().take(4096).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        });
        self.synced_lyrics = lyrics.is_synced().then(|| text.into());
    }

    pub fn lyrics(&self) -> Lyrics {
        if self.instrumental {
            return Lyrics::default();
        }
        let mut lyrics = self
            .synced_lyrics
            .as_deref()
            .map(super::lrc::parse)
            .filter(|l| l.is_synced())
            .unwrap_or_else(|| super::lrc::plain(self.plain_lyrics.as_deref().unwrap_or_default()));
        lyrics.display_title = Some(self.track_name.clone());
        lyrics.display_artist = Some(self.artist_name.clone());
        lyrics
    }

    pub fn usable(&self) -> bool {
        self.instrumental || !self.lyrics().lines.is_empty()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProviderError {
    Unavailable,
    InvalidResponse,
    RateLimited { retry_seconds: u64 },
}

pub(crate) trait LyricsProvider {
    fn lookup(
        &self,
        query: &Query,
    ) -> impl std::future::Future<Output = Result<Option<Record>, ProviderError>> + Send;
    fn search(
        &self,
        query: &Query,
    ) -> impl std::future::Future<Output = Result<Vec<Record>, ProviderError>> + Send;
    fn fetch(
        &self,
        id: u64,
        query: Option<&Query>,
    ) -> impl std::future::Future<Output = Result<Option<Record>, ProviderError>> + Send;
}

pub(super) fn numeric_id(value: &serde_json::Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|id| *id > 0)
}
