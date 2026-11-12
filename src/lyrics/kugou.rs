//! Public lyric candidates and UTF-8 LRC downloads with transient access keys.

use super::{
    matching,
    provider::{numeric_id, LyricsProvider, ProviderError, ProviderId, Query, Record},
    transport::{self, RequestGate},
};
use crate::models::network::NetworkRoute;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::Value;

#[derive(Clone)]
pub(crate) struct Kugou {
    client: reqwest::Client,
    base: String,
    gate: RequestGate,
}

struct Candidate {
    record: Record,
    access_key: String,
}

impl Kugou {
    pub fn new(route: &NetworkRoute, gate: RequestGate) -> Result<Self, ProviderError> {
        Ok(Self {
            client: transport::client(route)?,
            base: "https://lyrics.kugou.com".into(),
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
            None,
            false,
        )
        .await?
        .ok_or(ProviderError::Unavailable)?;
        match body.get("status").and_then(Value::as_u64) {
            Some(200) => {}
            Some(_) => return Err(ProviderError::Unavailable),
            None => return Err(ProviderError::InvalidResponse),
        }
        Ok(body)
    }

    async fn candidates(&self, query: &Query) -> Result<Vec<Candidate>, ProviderError> {
        let mut params = vec![
            ("ver".into(), "1".into()),
            ("man".into(), "yes".into()),
            ("client".into(), "pc".into()),
            (
                "keyword".into(),
                format!("{} {}", query.artist.trim(), query.title.trim())
                    .trim()
                    .into(),
            ),
        ];
        if let Some(duration) = query.duration {
            params.push(("duration".into(), (u64::from(duration) * 1000).to_string()));
        }
        let body = self.request("/search", params).await?;
        let candidates = body
            .get("candidates")
            .and_then(Value::as_array)
            .ok_or(ProviderError::InvalidResponse)?;
        candidates
            .iter()
            .take(20)
            .map(|candidate| {
                let parsed = || {
                    Some(Candidate {
                        record: Record {
                            id: numeric_id(candidate.get("id")?)?,
                            source: ProviderId::Kugou,
                            track_name: candidate.get("song")?.as_str()?.into(),
                            artist_name: candidate.get("singer")?.as_str()?.into(),
                            duration: candidate
                                .get("duration")
                                .and_then(Value::as_f64)
                                .unwrap_or_default()
                                / 1000.0,
                            ..Default::default()
                        },
                        access_key: candidate.get("accesskey")?.as_str()?.into(),
                    })
                };
                parsed()
                    .filter(|candidate| !candidate.access_key.is_empty())
                    .ok_or(ProviderError::InvalidResponse)
            })
            .collect()
    }

    async fn download(&self, candidate: Candidate) -> Result<Option<Record>, ProviderError> {
        let body = self
            .request(
                "/download",
                vec![
                    ("ver".into(), "1".into()),
                    ("client".into(), "pc".into()),
                    ("id".into(), candidate.record.id.to_string()),
                    ("accesskey".into(), candidate.access_key),
                    ("fmt".into(), "lrc".into()),
                    ("charset".into(), "utf8".into()),
                ],
            )
            .await?;
        if body
            .get("id")
            .is_some_and(|id| numeric_id(id) != Some(candidate.record.id))
        {
            return Err(ProviderError::InvalidResponse);
        }
        let bytes = STANDARD
            .decode(
                body.get("content")
                    .and_then(Value::as_str)
                    .ok_or(ProviderError::InvalidResponse)?,
            )
            .map_err(|_| ProviderError::InvalidResponse)?;
        let text = String::from_utf8(bytes).map_err(|_| ProviderError::InvalidResponse)?;
        let mut record = candidate.record;
        record.set_text(&text);
        Ok(record.usable().then_some(record))
    }
}

impl LyricsProvider for Kugou {
    async fn lookup(&self, _: &Query) -> Result<Option<Record>, ProviderError> {
        Ok(None)
    }

    async fn search(&self, query: &Query) -> Result<Vec<Record>, ProviderError> {
        let mut candidates = self.candidates(query).await?;
        let mut metadata: Vec<_> = candidates
            .iter()
            .map(|candidate| candidate.record.clone())
            .collect();
        matching::rank_metadata(query, &mut metadata);
        candidates.sort_by_key(|candidate| {
            metadata
                .iter()
                .position(|record| record.id == candidate.record.id)
                .unwrap_or(usize::MAX)
        });
        let mut records = Vec::new();
        for candidate in candidates.into_iter().take(5) {
            // Incomplete hydration cannot establish a miss or a unique match.
            if let Some(record) = self.download(candidate).await? {
                records.push(record);
            }
        }
        Ok(records)
    }

    async fn fetch(&self, id: u64, query: Option<&Query>) -> Result<Option<Record>, ProviderError> {
        // Access keys are obtained from a fresh search, never stored in caches.
        let candidate = self
            .candidates(query.ok_or(ProviderError::Unavailable)?)
            .await?
            .into_iter()
            .find(|candidate| candidate.record.id == id);
        match candidate {
            Some(candidate) => self.download(candidate).await,
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests;
