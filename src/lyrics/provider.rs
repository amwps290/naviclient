//! Stable provider DTOs; no audio addresses, account names, or credentials.

use crate::models::{Lyrics, Song};
use crate::source::ProviderKind;
use serde::{Deserialize, Serialize};

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

impl Record {
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
    ) -> impl std::future::Future<Output = Result<Option<Record>, ProviderError>> + Send;
}
