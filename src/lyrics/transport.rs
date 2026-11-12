//! Bounded JSON requests, per-service sequential spacing and cooldown.

use super::provider::ProviderError;
use crate::models::network::NetworkRoute;
use reqwest::StatusCode;
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::{sync::Mutex, time::Instant};

#[derive(Default)]
struct Gate {
    next: Option<Instant>,
    cooldown: Option<Instant>,
}

/// Reserve spacing even when a response fails or the request is cancelled.
struct RequestSpacing<'a>(&'a mut Gate);

impl Drop for RequestSpacing<'_> {
    fn drop(&mut self) {
        self.0.next = Some(Instant::now() + Duration::from_millis(300));
    }
}

#[derive(Clone, Default)]
pub(crate) struct RequestGate(Arc<Mutex<Gate>>);

pub(super) fn client(route: &NetworkRoute) -> Result<reqwest::Client, ProviderError> {
    let builder = reqwest::Client::builder()
        .user_agent(concat!(
            "Navidrome-Client/",
            env!("CARGO_PKG_VERSION"),
            " (https://github.com/amwps290/naviclient)"
        ))
        .referer(false)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(8));
    crate::source::resource::clients::route_async(builder, route)
        .and_then(|builder| builder.build().map_err(Into::into))
        .map_err(|_| ProviderError::Unavailable)
}

pub(super) async fn request<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    gate: &RequestGate,
    url: String,
    params: &[(String, String)],
    referer: Option<&'static str>,
    strict_json: bool,
) -> Result<Option<T>, ProviderError> {
    let mut gate = gate.0.lock().await;
    if let Some(until) = gate.cooldown.filter(|until| *until > Instant::now()) {
        return Err(ProviderError::RateLimited {
            retry_seconds: until
                .saturating_duration_since(Instant::now())
                .as_secs()
                .saturating_add(1),
        });
    }
    if let Some(next) = gate.next {
        tokio::time::sleep_until(next).await;
    }
    let spacing = RequestSpacing(&mut gate);
    let response = client
        .get(url)
        .query(params)
        .headers(
            referer
                .map(|value| {
                    let mut headers = reqwest::header::HeaderMap::new();
                    headers.insert(
                        reqwest::header::REFERER,
                        reqwest::header::HeaderValue::from_static(value),
                    );
                    headers
                })
                .unwrap_or_default(),
        )
        .send()
        .await
        .map_err(|_| ProviderError::Unavailable)?;
    let status = response.status();
    if status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::SERVICE_UNAVAILABLE {
        let retry = retry_after(
            response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|h| h.to_str().ok()),
            SystemTime::now(),
        );
        let seconds = retry.unwrap_or(30);
        let delay =
            Duration::from_secs(seconds) + Duration::from_millis(rand::random::<u8>() as u64);
        spacing.0.cooldown = Instant::now()
            .checked_add(delay)
            .or_else(|| Instant::now().checked_add(Duration::from_secs(86400 * 365)));
        return Err(
            if status == StatusCode::SERVICE_UNAVAILABLE && retry.is_none() {
                ProviderError::Unavailable
            } else {
                ProviderError::RateLimited {
                    retry_seconds: seconds,
                }
            },
        );
    }
    if status == StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(ProviderError::Unavailable);
    }
    // Status is checked first because edge errors may be HTML rather than JSON.
    if strict_json
        && !response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .is_some_and(|h| h.to_ascii_lowercase().contains("application/json"))
    {
        return Err(ProviderError::InvalidResponse);
    }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| ProviderError::Unavailable)?
    {
        if bytes.len().saturating_add(chunk.len()) > 4 * 1024 * 1024 {
            return Err(ProviderError::InvalidResponse);
        }
        bytes.extend_from_slice(&chunk);
    }
    // Hold the gate through the body to keep requests truly sequential.
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| ProviderError::InvalidResponse)
}

pub(super) fn retry_after(value: Option<&str>, now: SystemTime) -> Option<u64> {
    let value = value?;
    value.trim().parse::<u64>().ok().or_else(|| {
        chrono::DateTime::parse_from_rfc2822(value)
            .ok()
            .map(|date| {
                let seconds = date
                    .timestamp()
                    .saturating_sub(chrono::DateTime::<chrono::Utc>::from(now).timestamp());
                u64::try_from(seconds).unwrap_or(0)
            })
    })
}
