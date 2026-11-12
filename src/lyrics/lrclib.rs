//! LRCLIB v1 transport with shared sequential throttling and service cooldown.

use super::provider::{LyricsProvider, ProviderError, ProviderId, Query, Record};
#[cfg(test)]
use super::transport::retry_after;
use crate::models::network::NetworkRoute;
#[cfg(test)]
use std::time::Duration;
#[cfg(test)]
use tokio::time::Instant;

pub(crate) use super::transport::RequestGate;

#[derive(Clone)]
pub(crate) struct Lrclib {
    client: reqwest::Client,
    base: String,
    gate: RequestGate,
}

impl Lrclib {
    #[cfg(test)]
    pub(crate) fn test_client(base: String) -> Self {
        let mut client = Self::new(&NetworkRoute::Direct, RequestGate::default()).unwrap();
        client.base = base;
        client
    }
    pub fn new(route: &NetworkRoute, gate: RequestGate) -> Result<Self, ProviderError> {
        let client = super::transport::client(route)?;
        Ok(Self {
            client,
            base: "https://lrclib.net".into(),
            gate,
        })
    }

    async fn request<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        params: &[(String, String)],
    ) -> Result<Option<T>, ProviderError> {
        super::transport::request(
            &self.client,
            &self.gate,
            format!("{}{path}", self.base),
            params,
            None,
            true,
        )
        .await
    }
}

fn params(query: &Query, duration: bool) -> Vec<(String, String)> {
    let mut params = vec![("track_name".into(), query.title.trim().into())];
    if !query.artist.trim().is_empty() {
        params.push(("artist_name".into(), query.artist.trim().into()));
    }
    if !query.album.trim().is_empty() {
        params.push(("album_name".into(), query.album.trim().into()));
    }
    if duration {
        if let Some(duration) = query.duration {
            params.push(("duration".into(), duration.to_string()));
        }
    }
    params
}

impl LyricsProvider for Lrclib {
    async fn lookup(&self, query: &Query) -> Result<Option<Record>, ProviderError> {
        let record: Option<Record> = self.request("/api/get", &params(query, true)).await?;
        Ok(record.map(|mut record| {
            record.source = ProviderId::Lrclib;
            record
        }))
    }
    async fn search(&self, query: &Query) -> Result<Vec<Record>, ProviderError> {
        let mut records: Vec<Record> = self
            .request("/api/search", &params(query, false))
            .await?
            .unwrap_or_default();
        records.truncate(20);
        for record in &mut records {
            record.source = ProviderId::Lrclib;
        }
        Ok(records)
    }
    async fn fetch(&self, id: u64, _: Option<&Query>) -> Result<Option<Record>, ProviderError> {
        let record: Option<Record> = self.request(&format!("/api/get/{id}"), &[]).await?;
        Ok(record.map(|mut record| {
            record.source = ProviderId::Lrclib;
            record
        }))
    }
}

#[cfg(test)]
mod tests;
