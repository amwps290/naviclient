//! QQ 音乐公开歌单拉取。
//!
//! 使用当前网页的 `musicu` 接口，只读取用于曲库匹配的元数据，不请求音源、
//! 不需要登录。`song_num` 取足够大的值避免大型歌单被默认分页截断；响应中的
//! 声明数量仍用于完整性校验。

use std::time::Duration;

use anyhow::{anyhow, Result};
use rust_i18n::t;
use serde_json::Value;

use super::http::json;
use super::ImportedPlaylist;

mod response;

use response::parse_response;

const MUSICU_API: &str = "https://u.y.qq.com/cgi-bin/musicu.fcg";
const REFERER: &str = "https://y.qq.com/";

pub(super) async fn fetch(http: &reqwest::Client, id: &str) -> Result<ImportedPlaylist> {
    fetch_from(http, id, MUSICU_API).await
}

async fn fetch_from(http: &reqwest::Client, id: &str, endpoint: &str) -> Result<ImportedPlaylist> {
    request_musicu(http, id, endpoint)
        .await
        .and_then(|body| parse_response(&body))
}

async fn request_musicu(http: &reqwest::Client, id: &str, endpoint: &str) -> Result<Value> {
    let disstid = id
        .parse::<u64>()
        .map_err(|_| anyhow!(t!("Unrecognized playlist link.")))?;
    let payload = serde_json::json!({
        "comm": {"ct": 24, "cv": 0},
        "req": {
            "module": "music.srfDissInfo.aiDissInfo",
            "method": "uniform_get_Dissinfo",
            "param": {
                "disstid": disstid,
                "enc_host_uin": "",
                "tag": 1,
                "userinfo": 1,
                "song_begin": 0,
                "song_num": 10000
            }
        }
    });
    json(
        http.post(endpoint)
            .json(&payload)
            .header(reqwest::header::REFERER, REFERER)
            .timeout(Duration::from_secs(30)),
    )
    .await
}

#[cfg(test)]
mod tests;
