//! 第三方歌单导入：解析分享链接并拉取歌单元数据。
//!
//! 设计为“数据源 trait 化”的增量结构：`PlaylistSource` 枚举每个平台一个
//! 变体，链接识别（`resolve`）与拉取（`fetch`）都收口在这里；上层
//! （`src/app/import.rs`）只面对统一的 `ImportedPlaylist`。
//!
//! P1 支持网易云音乐（公开歌单，无需登录、无需加密签名，走 v6 详情接口，
//! 再按 `trackIds` 补齐歌曲元数据）。QQ 音乐等平台后续按同样方式追加即可。
//!
//! 只取元数据用于匹配用户自己的 Navidrome 曲库，不获取音源。

mod matcher;
mod netease;

pub use matcher::match_playlist;

use anyhow::{anyhow, bail, Result};
use rust_i18n::t;
use std::sync::OnceLock;
use std::time::Duration;
use url::Url;

use crate::models::Song;

/// 匹配结果：歌曲与失败标记都和导入曲目一一对应。失败标记用于区分
/// “确实没有合适候选”和“服务器请求失败，未能完成检查”。
#[derive(Debug)]
pub struct MatchedSongs {
    pub songs: Vec<Option<Song>>,
    pub failures: Vec<bool>,
}

#[cfg(test)]
mod tests;

/// 浏览器 UA：网易/QQ 的公开接口都要求非空 UA，否则直接拒绝。
const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

/// 从外部平台导入的一行曲目元数据（只含匹配需要的字段）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportedTrack {
    pub title: String,
    /// 可能多位（feat / 合唱），首元素视为主艺人。
    pub artists: Vec<String>,
    pub album: String,
    pub duration_ms: u32,
}

/// 解析后的外部歌单。
#[derive(Clone, Debug)]
pub struct ImportedPlaylist {
    pub name: String,
    pub tracks: Vec<ImportedTrack>,
    /// 网易云仍在歌单中声明、但歌曲详情接口已无法公开返回的曲目数。
    pub unavailable_tracks: usize,
}

/// 一个可拉取的外部歌单源。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlaylistSource {
    /// 网易云音乐歌单 id。
    Netease { id: String },
}

impl PlaylistSource {
    /// 解析已知的平台直链（不跟随短链）。识别失败返回 None。
    ///
    /// 网易云的网页链接把路径放在 `#` 后面（`music.163.com/#/playlist?id=x`），
    /// 因此同时校验普通路径和 fragment 路径；域名必须精确匹配，避免把
    /// 路径中碰巧包含平台域名的任意网址当成歌单。
    pub fn parse_known_url(url: &str) -> Option<Self> {
        let parsed = parse_http_url(url)?;
        if parsed.host_str() != Some("music.163.com") {
            return None;
        }
        let id = playlist_id(&parsed)?;
        Some(Self::Netease { id })
    }

    /// 拉取歌单元数据。
    pub async fn fetch(&self, http: &reqwest::Client) -> Result<ImportedPlaylist> {
        match self {
            Self::Netease { id } => netease::fetch(http, id).await,
        }
    }
}

/// 解析用户粘贴的分享文本：抽出链接、必要时展开短链（163cn.tv），
/// 识别出歌单源。
pub async fn resolve(input: &str, http: &reqwest::Client) -> Result<PlaylistSource> {
    let url = extract_url(input).ok_or_else(|| anyhow!(t!("Unrecognized playlist link.")))?;
    if let Some(source) = PlaylistSource::parse_known_url(url) {
        return Ok(source);
    }
    // 短链（分享 App 生成）：302 落到 music.163.com 的正式链接上。
    // 客户端重定向策略只允许 163cn.tv / music.163.com，最终地址再次解析。
    if is_netease_short_url(url) {
        let response = http
            .get(url)
            .header(reqwest::header::USER_AGENT, BROWSER_UA)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|error| anyhow!(t!("Playlist request failed: %{error}", error = error)))?;
        let status = response.status();
        if !status.is_success() {
            bail!(t!(
                "Playlist request failed: %{error}",
                error = format!("HTTP {status}")
            ));
        }
        let final_url = response.url().to_string();
        return PlaylistSource::parse_known_url(&final_url)
            .ok_or_else(|| anyhow!(t!("Unrecognized playlist link.")));
    }
    bail!(t!("Unrecognized playlist link."))
}

/// 第三方请求共用的 HTTP 客户端（带浏览器 UA、连接超时）。
pub fn http_client() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .user_agent(BROWSER_UA)
                .connect_timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    if attempt.previous().len() >= 5 {
                        return attempt.error("too many playlist redirects");
                    }
                    if is_allowed_import_host(attempt.url()) {
                        attempt.follow()
                    } else {
                        attempt.error("playlist redirect host is not allowed")
                    }
                }))
                .build()
                .expect("playlist import HTTP client")
        })
        .clone()
}

/// 从任意粘贴文本里抽出第一个 http(s) 链接。
///
/// App 分享文案形如 `……https://163cn.tv/ab12cd 来自@网易云音乐`，
/// 链接在 CJK 字符处截断；也兼容被括号/引号包裹的形式。
pub fn extract_url(text: &str) -> Option<&str> {
    let start = [text.find("http://"), text.find("https://")]
        .into_iter()
        .flatten()
        .min()?;
    let mut end = start;
    for (offset, character) in text[start..].char_indices() {
        if is_url_character(character) {
            end = start + offset + character.len_utf8();
        } else {
            break;
        }
    }
    let url = text[start..end].trim_end_matches(['.', ')', '>', '"', '\'']);
    if url.len() > "https://".len() {
        Some(url)
    } else {
        None
    }
}

/// RFC 3986 允许的 URL 字符（单引号除外，避免吃进文案引号）。
fn is_url_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || "-._~:/?#[]@!$&()*+,;=%".contains(character)
}

fn parse_http_url(raw: &str) -> Option<Url> {
    let parsed = Url::parse(raw).ok()?;
    matches!(parsed.scheme(), "http" | "https").then_some(parsed)
}

fn is_netease_short_url(raw: &str) -> bool {
    parse_http_url(raw).is_some_and(|url| url.host_str() == Some("163cn.tv"))
}

fn is_allowed_import_host(url: &Url) -> bool {
    matches!(url.host_str(), Some("163cn.tv" | "music.163.com"))
}

/// 从已经确认是歌单的路由中取 id。hash 路由优先于外层页面 query，避免
/// `song?id=111#/playlist?id=222` 误取到不属于歌单路由的 111。
fn playlist_id(url: &Url) -> Option<String> {
    if let Some(fragment) = url.fragment() {
        let (path, query) = fragment.split_once('?').unwrap_or((fragment, ""));
        if path.trim_start_matches('/') == "playlist" {
            return query_id(query);
        }
    }
    let path_is_playlist = url
        .path_segments()
        .and_then(|mut segments| segments.next_back())
        == Some("playlist");
    path_is_playlist.then(|| query_id(url.query().unwrap_or_default()))?
}

fn query_id(query: &str) -> Option<String> {
    url::form_urlencoded::parse(query.as_bytes()).find_map(|(key, value)| {
        (key == "id" && !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| value.into_owned())
    })
}
