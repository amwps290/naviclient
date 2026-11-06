//! 网易云音乐歌单拉取（P1 数据源）。
//!
//! 走公开 v6 详情接口取得可信的 `trackCount` / `trackIds`，再分批调用
//! `api/song/detail` 补齐歌曲元数据。无需登录或 weapi 加密，请求带浏览器
//! UA 与 Referer。已下架或对未登录请求不可见的歌曲会计数后跳过；除此之外
//! 仍用 `trackCount` 校验真正的截断或解析损坏。

use std::time::Duration;

use anyhow::{anyhow, Result};
use rust_i18n::t;
use serde_json::Value;

use super::http::json;
use super::ImportedPlaylist;

mod hydration;
mod response;

use hydration::{TrackHydration, SONG_BATCH_SIZE};
use response::{ensure_success_code, parse_response_with_unavailable};

const PLAYLIST_API: &str = "https://music.163.com/api/v6/playlist/detail";
const SONG_DETAIL_API: &str = "https://music.163.com/api/song/detail";
const REFERER: &str = "https://music.163.com/";

pub(super) async fn fetch(http: &reqwest::Client, id: &str) -> Result<ImportedPlaylist> {
    fetch_from(http, id, PLAYLIST_API, SONG_DETAIL_API).await
}

async fn fetch_from(
    http: &reqwest::Client,
    id: &str,
    playlist_api: &str,
    song_detail_api: &str,
) -> Result<ImportedPlaylist> {
    // v6 的 trackCount / trackIds 才是权威数量。旧接口对部分大型歌单会
    // 静默截成 10 首并伪造 trackCount，因此不能作为可信兜底。
    let mut body = request_json(
        http,
        playlist_api,
        &[("id", id), ("n", "100000"), ("s", "0")],
    )
    .await?;
    ensure_success_code(&body)?;
    let unavailable_tracks = hydrate_tracks(http, song_detail_api, &mut body).await?;
    parse_response_with_unavailable(&body, unavailable_tracks)
}

async fn request_json(
    http: &reqwest::Client,
    endpoint: &str,
    query: &[(&str, &str)],
) -> Result<Value> {
    json(
        http.get(endpoint)
            .query(query)
            .header(reqwest::header::REFERER, REFERER)
            .timeout(Duration::from_secs(30)),
    )
    .await
}

async fn hydrate_tracks(http: &reqwest::Client, endpoint: &str, body: &mut Value) -> Result<usize> {
    let playlist = body
        .get("playlist")
        .ok_or_else(|| anyhow!(t!("Invalid playlist response.")))?;
    let Some(mut hydration) = TrackHydration::new(playlist) else {
        return Ok(0);
    };
    while let Some(batch) = hydration.next_batch() {
        let songs = fetch_song_details(http, endpoint, batch).await?;
        hydration.include(songs);
    }
    // Retry only the first pass's missing IDs once before counting unavailable
    // tracks. HTTP and API failures still abort the import.
    let first_missing = hydration.missing_ids();
    for batch in first_missing.chunks(SONG_BATCH_SIZE) {
        let songs = fetch_song_details(http, endpoint, batch).await?;
        hydration.include(songs);
    }
    let (tracks, unavailable) = hydration.finish();
    body["playlist"]["tracks"] = Value::Array(tracks);
    Ok(unavailable)
}

async fn fetch_song_details(
    http: &reqwest::Client,
    endpoint: &str,
    ids: &[String],
) -> Result<Vec<Value>> {
    let ids = format!("[{}]", ids.join(","));
    let detail = request_json(http, endpoint, &[("ids", ids.as_str())]).await?;
    ensure_success_code(&detail)?;
    Ok(detail
        .get("songs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default())
}

#[cfg(test)]
mod tests;
