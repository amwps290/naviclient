//! Anonymous metadata and LRC retrieval; no audio, cookies or account credentials.

use super::{
    matching,
    provider::{numeric_id, LyricsProvider, ProviderError, ProviderId, Query, Record},
    transport::{self, RequestGate},
};
use crate::models::network::NetworkRoute;
use serde_json::Value;

#[derive(Clone)]
pub(crate) struct Netease {
    client: reqwest::Client,
    base: String,
    gate: RequestGate,
}

impl Netease {
    pub fn new(route: &NetworkRoute, gate: RequestGate) -> Result<Self, ProviderError> {
        Ok(Self {
            client: transport::client(route)?,
            base: "https://music.163.com".into(),
            gate,
        })
    }

    #[cfg(test)]
    pub(crate) fn test_client(base: String) -> Self {
        Self {
            base,
            ..Self::new(&NetworkRoute::Direct, RequestGate::default()).unwrap()
        }
    }

    async fn request(
        &self,
        path: &str,
        params: Vec<(String, String)>,
    ) -> Result<Value, ProviderError> {
        let body: Value = transport::request(
            &self.client,
            &self.gate,
            format!("{}{path}", self.base),
            &params,
            Some("https://music.163.com/"),
            false,
        )
        .await?
        .ok_or(ProviderError::Unavailable)?;
        match body.get("code").and_then(Value::as_u64) {
            Some(200) => {}
            Some(_) => return Err(ProviderError::Unavailable),
            None => return Err(ProviderError::InvalidResponse),
        }
        Ok(body)
    }

    async fn lyrics(&self, mut record: Record) -> Result<Option<Record>, ProviderError> {
        let body = self
            .request(
                "/api/song/lyric",
                vec![
                    ("id".into(), record.id.to_string()),
                    ("lv".into(), "-1".into()),
                    ("kv".into(), "-1".into()),
                    ("tv".into(), "-1".into()),
                    ("rv".into(), "-1".into()),
                ],
            )
            .await?;
        // Absence of lyrics is not enough to infer an instrumental recording.
        let text = body
            .pointer("/lrc/lyric")
            .and_then(Value::as_str)
            .unwrap_or_default();
        record.set_text(text);
        Ok(record.usable().then_some(record))
    }
}

fn metadata(song: &Value) -> Option<Record> {
    let artists = song.get("artists").or_else(|| song.get("ar"))?.as_array()?;
    Some(Record {
        id: numeric_id(song.get("id")?)?,
        source: ProviderId::Netease,
        track_name: song.get("name")?.as_str()?.into(),
        artist_name: artists
            .iter()
            .filter_map(|artist| artist.get("name")?.as_str())
            .collect::<Vec<_>>()
            .join(" / "),
        album_name: song
            .get("album")
            .or_else(|| song.get("al"))
            .and_then(|album| album.get("name"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .into(),
        duration: song
            .get("duration")
            .or_else(|| song.get("dt"))
            .and_then(Value::as_f64)
            .unwrap_or_default()
            / 1000.0,
        ..Default::default()
    })
}

impl LyricsProvider for Netease {
    async fn lookup(&self, _: &Query) -> Result<Option<Record>, ProviderError> {
        Ok(None)
    }

    async fn search(&self, query: &Query) -> Result<Vec<Record>, ProviderError> {
        let body = self
            .request(
                "/api/search/get/web",
                vec![
                    (
                        "s".into(),
                        format!("{} {}", query.title.trim(), query.artist.trim())
                            .trim()
                            .into(),
                    ),
                    ("type".into(), "1".into()),
                    ("limit".into(), "10".into()),
                    ("offset".into(), "0".into()),
                ],
            )
            .await?;
        let result = body.get("result").ok_or(ProviderError::InvalidResponse)?;
        let songs = match result.get("songs").and_then(Value::as_array) {
            Some(songs) => songs,
            None if result.get("songCount").and_then(Value::as_u64) == Some(0) => {
                return Ok(Vec::new())
            }
            None => return Err(ProviderError::InvalidResponse),
        };
        let mut candidates: Vec<_> = songs
            .iter()
            .take(10)
            .map(|song| metadata(song).ok_or(ProviderError::InvalidResponse))
            .collect::<Result<_, _>>()?;
        matching::rank_metadata(query, &mut candidates);
        let mut records = Vec::new();
        for record in candidates.into_iter().take(5) {
            // Incomplete hydration cannot establish a miss or a unique match.
            if let Some(record) = self.lyrics(record).await? {
                records.push(record);
            }
        }
        Ok(records)
    }

    async fn fetch(&self, id: u64, _: Option<&Query>) -> Result<Option<Record>, ProviderError> {
        let body = self
            .request("/api/song/detail", vec![("ids".into(), format!("[{id}]"))])
            .await?;
        let songs = body
            .get("songs")
            .and_then(Value::as_array)
            .ok_or(ProviderError::InvalidResponse)?;
        let record = songs
            .iter()
            .filter_map(metadata)
            .find(|record| record.id == id);
        match record {
            Some(record) => self.lyrics(record).await,
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests;
