//! JSON request failures shared by metadata sources. API codes remain source-specific.

use anyhow::{anyhow, bail, Result};
use reqwest::RequestBuilder;
use rust_i18n::t;
use serde_json::Value;

pub(super) async fn json(request: RequestBuilder) -> Result<Value> {
    let response = request
        .send()
        .await
        .map_err(reqwest::Error::without_url)
        .map_err(|error| anyhow!(t!("Playlist request failed: %{error}", error = error)))?;
    let status = response.status();
    if !status.is_success() {
        bail!(t!(
            "Playlist request failed: %{error}",
            error = format!("HTTP {status}")
        ));
    }
    response
        .json()
        .await
        .map_err(|error| anyhow!(t!("Playlist request failed: %{error}", error = error)))
}
