//! OpenSubsonic provider, authentication, and shared HTTP transport.

use rust_i18n::t;
use std::fmt::Write;

use anyhow::{anyhow, Context, Result};
use md5::{Digest, Md5};
use rand::Rng;
use serde_json::Value;
use url::Url;

mod library;
mod lyrics;
mod playback;
mod playlists;
mod server;

#[derive(Clone)]
pub struct OpenSubsonicProvider {
    client: reqwest::Client,
    base_url: String,
    username: String,
    password: String,
}

impl OpenSubsonicProvider {
    pub fn new(server_url: &str, username: &str, password: &str) -> Result<Self> {
        let base_url = if server_url.contains("://") {
            server_url.trim_end_matches('/').to_string()
        } else {
            format!("http://{}", server_url.trim_end_matches('/'))
        };

        let probe = Url::parse(&format!("{base_url}/rest/ping.view"))?;
        if !matches!(probe.scheme(), "http" | "https") {
            return Err(anyhow!(t!("server URL must use http or https")));
        }

        Ok(Self {
            client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()?,
            base_url,
            username: username.to_string(),
            password: password.to_string(),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn auth_params(&self) -> (String, String) {
        let mut rng = rand::thread_rng();
        let salt: String = (0..16)
            .map(|_| {
                let digit = rng.gen_range(0..10u8);
                char::from(b'0' + digit)
            })
            .collect();
        let mut hasher = Md5::new();
        hasher.update(self.password.as_bytes());
        hasher.update(salt.as_bytes());
        let token = format!("{:x}", hasher.finalize());
        (token, salt)
    }

    fn url_for(&self, view: &str, params: &[(&str, &str)]) -> Result<String> {
        let (token, salt) = self.auth_params();
        let mut url = Url::parse(&format!("{}/rest/{}.view", self.base_url, view))?;
        let mut query = url.query_pairs_mut();
        query.append_pair("u", &self.username);
        query.append_pair("t", &token);
        query.append_pair("s", &salt);
        query.append_pair("v", "1.16.1");
        query.append_pair("c", "navidrome-client");
        query.append_pair("f", "json");
        for (key, value) in params {
            query.append_pair(key, value);
        }
        drop(query);
        Ok(url.to_string())
    }

    async fn get_json(&self, view: &str, params: &[(&str, &str)]) -> Result<Value> {
        self.request_json(view, params, false).await
    }

    async fn request_json(&self, view: &str, params: &[(&str, &str)], post: bool) -> Result<Value> {
        // Large playlists belong in a form body, not in a URL that a proxy may truncate.
        let request = if post {
            self.client.post(self.url_for(view, &[])?).form(params)
        } else {
            self.client.get(self.url_for(view, params)?)
        };
        let response = request
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(reqwest::Error::without_url)
            .with_context(|| t!("request to %{view} failed", view = view))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .with_context(|| t!("read response from %{view} failed", view = view))?;

        if !status.is_success() {
            return Err(anyhow!(
                "server returned HTTP {status}: {}",
                text.chars().take(240).collect::<String>()
            ));
        }

        let root: Value = serde_json::from_str(&text).with_context(|| {
            format!(
                "invalid JSON from {view}: {}",
                text.chars().take(160).collect::<String>()
            )
        })?;
        let body = root
            .get("subsonic-response")
            .context("server response is not a Subsonic response")?;

        if body.get("status").and_then(Value::as_str) != Some("ok") {
            let code = body
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(Value::as_i64);
            let message = body
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("unknown server error");
            return Err(anyhow!("Subsonic error {}: {message}", code.unwrap_or(-1)));
        }

        Ok(body.clone())
    }

    pub async fn get_bytes(&self, url: &str) -> Result<Vec<u8>> {
        let response = self.client.get(url).send().await?.error_for_status()?;
        Ok(response.bytes().await?.to_vec())
    }
}

fn parse_array<T>(value: &Value, key: &str) -> Result<Vec<T>>
where
    T: serde::de::DeserializeOwned,
{
    let mut out = Vec::new();
    for item in value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        out.push(serde_json::from_value(item.clone())?);
    }
    Ok(out)
}

pub fn format_duration(seconds: Option<i32>) -> String {
    let seconds = seconds.unwrap_or(0).max(0) as u64;
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        let mut out = String::new();
        let _ = write!(out, "{hours}:{minutes:02}:{secs:02}");
        out
    } else {
        let mut out = String::new();
        let _ = write!(out, "{minutes}:{secs:02}");
        out
    }
}

#[cfg(test)]
mod tests;
