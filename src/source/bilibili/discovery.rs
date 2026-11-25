//! Bounded Bilibili video discovery. Search results are metadata, not audio URLs.

use super::{
    client::{BilibiliClient, BilibiliError, Envelope},
    BilibiliProvider,
};
use crate::{models::Song, source::SourceId};
use serde::Deserialize;
use std::collections::HashSet;

#[cfg(test)]
mod tests;
mod wbi;

pub const MAX_SEARCH_PAGES: u32 = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VideoId {
    Bvid(String),
    Aid(u64),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoInput {
    pub id: Option<VideoId>,
    pub short: Option<url::Url>,
    pub part: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct VideoSummary {
    pub bvid: String,
    pub title: String,
    pub uploader: String,
    pub cover: Option<String>,
    pub duration: u32,
}
#[derive(Clone, Debug)]
pub struct SearchPage {
    pub source: SourceId,
    pub keyword: String,
    pub page: u32,
    pub pages: u32,
    pub total: u64,
    pub videos: Vec<VideoSummary>,
}
#[derive(Clone, Debug)]
pub struct VideoTracks {
    pub source: SourceId,
    pub title: String,
    pub uploader: String,
    pub cover: Option<String>,
    pub songs: Vec<Song>,
    pub selected: usize,
}

pub fn parse_video_input(value: &str) -> Result<Option<VideoInput>, BilibiliError> {
    let value = value.trim();
    fn id(value: &str) -> Option<VideoId> {
        if super::library::valid_bvid(value) {
            return Some(VideoId::Bvid(value.into()));
        }
        let raw = value
            .strip_prefix("av")
            .or_else(|| value.strip_prefix("AV"))?;
        if raw.is_empty() || !raw.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        raw.parse::<u64>()
            .ok()
            .filter(|id| *id > 0)
            .map(VideoId::Aid)
    }
    if let Some(id) = id(value) {
        return Ok(Some(VideoInput {
            id: Some(id),
            short: None,
            part: None,
        }));
    }
    let looks_like_id = (value.starts_with("BV") && !value.contains(char::is_whitespace))
        || value
            .strip_prefix("av")
            .or_else(|| value.strip_prefix("AV"))
            .is_some_and(|raw| raw.bytes().all(|c| c.is_ascii_digit()));
    if looks_like_id {
        return Err(BilibiliError::InvalidVideoInput);
    }
    if !value.contains("://")
        && !value.starts_with("www.bilibili.com/")
        && !value.starts_with("bilibili.com/")
        && !value.starts_with("m.bilibili.com/")
        && !value.starts_with("b23.tv/")
    {
        return Ok(None);
    }
    let normalized = if value.contains("://") {
        value.to_string()
    } else {
        format!("https://{value}")
    };
    let mut url = url::Url::parse(&normalized).map_err(|_| BilibiliError::InvalidVideoInput)?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(BilibiliError::InvalidVideoInput);
    }
    url.set_scheme("https")
        .map_err(|_| BilibiliError::InvalidVideoInput)?;
    if short_url(&url) {
        return Ok(Some(VideoInput {
            id: None,
            short: Some(url),
            part: None,
        }));
    }
    if !matches!(
        url.host_str(),
        Some("www.bilibili.com" | "bilibili.com" | "m.bilibili.com")
    ) {
        return Err(BilibiliError::InvalidVideoInput);
    }
    let segments = url
        .path_segments()
        .ok_or(BilibiliError::InvalidVideoInput)?
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if segments.len() != 2 || segments[0] != "video" {
        return Err(BilibiliError::InvalidVideoInput);
    }
    let id = id(segments[1]).ok_or(BilibiliError::InvalidVideoInput)?;
    let parts = url
        .query_pairs()
        .filter(|(key, _)| key == "p")
        .collect::<Vec<_>>();
    if parts.len() > 1 {
        return Err(BilibiliError::InvalidVideoInput);
    }
    let part = parts
        .first()
        .map(|(_, value)| {
            value
                .parse::<usize>()
                .ok()
                .filter(|part| *part > 0)
                .map(|part| part - 1)
                .ok_or(BilibiliError::InvalidVideoInput)
        })
        .transpose()?;
    Ok(Some(VideoInput {
        id: Some(id),
        short: None,
        part,
    }))
}

pub(super) fn short_url(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("b23.tv")
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.path().trim_matches('/').len() <= 64
        && !url.path().trim_matches('/').is_empty()
        && url
            .path()
            .trim_matches('/')
            .bytes()
            .all(|c| c.is_ascii_alphanumeric())
}

fn plain_title(value: &str) -> String {
    let mut in_tag = false;
    let text = value
        .chars()
        .filter(|c| match c {
            '<' => {
                in_tag = true;
                false
            }
            '>' if in_tag => {
                in_tag = false;
                false
            }
            _ => !in_tag,
        })
        .collect::<String>();
    text.replace("&nbsp;", " ")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .trim()
        .to_string()
}
fn duration(value: &str) -> Option<u32> {
    let parts = value.split(':').collect::<Vec<_>>();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let mut total = 0_u32;
    for (index, part) in parts.iter().enumerate() {
        let number = part.trim().parse::<u32>().ok()?;
        if index > 0 && number >= 60 {
            return None;
        }
        total = total.checked_mul(60)?.checked_add(number)?;
    }
    (total <= i32::MAX as u32).then_some(total)
}

#[derive(Deserialize)]
struct WbiNav {
    #[serde(rename = "isLogin", default)]
    login: bool,
    #[serde(default)]
    mid: u64,
    wbi_img: Option<WbiImages>,
}
#[derive(Deserialize)]
struct WbiImages {
    img_url: String,
    sub_url: String,
}
#[derive(Deserialize)]
struct SearchData {
    #[serde(default)]
    page: Option<u32>,
    #[serde(rename = "numPages", default)]
    pages: u32,
    #[serde(rename = "numResults", default)]
    total: u64,
    #[serde(default)]
    result: Vec<SearchVideo>,
}
#[derive(Deserialize)]
struct SearchVideo {
    bvid: String,
    title: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    pic: String,
    duration: String,
}

impl BilibiliProvider {
    pub async fn search_videos(
        &self,
        keyword: &str,
        page: u32,
    ) -> Result<SearchPage, BilibiliError> {
        let keyword = keyword.trim();
        if keyword.is_empty() || keyword.len() > 512 || !(1..=MAX_SEARCH_PAGES).contains(&page) {
            return Err(BilibiliError::InvalidVideoInput);
        }
        let response = self
            .client
            .get(false, "/x/web-interface/nav", &[], Some(&self.session))
            .await?;
        let nav = BilibiliClient::json::<Envelope<WbiNav>>(response)
            .await?
            .into_data()?;
        if !nav.login {
            return Err(BilibiliError::NotAuthenticated);
        }
        if nav.mid != self.session.account_id() {
            return Err(BilibiliError::AccountMismatch);
        }
        let images = nav.wbi_img.ok_or(BilibiliError::InvalidResponse)?;
        let timestamp = u64::try_from(chrono::Utc::now().timestamp())
            .map_err(|_| BilibiliError::InvalidResponse)?;
        let signed = wbi::sign(
            &images.img_url,
            &images.sub_url,
            &[
                ("search_type", "video".into()),
                ("keyword", keyword.into()),
                ("page", page.to_string()),
                ("page_size", "20".into()),
            ],
            timestamp,
        )?;
        let params = signed
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect::<Vec<_>>();
        let response = self
            .client
            .get(
                false,
                "/x/web-interface/wbi/search/type",
                &params,
                Some(&self.session),
            )
            .await?;
        let data = BilibiliClient::json::<Envelope<SearchData>>(response)
            .await?
            .into_data()?;
        if data.page.is_some_and(|returned| returned != page)
            || data.result.len() > 50
            || (data.total > 0 && data.pages == 0)
        {
            return Err(BilibiliError::InvalidResponse);
        }
        let mut seen = HashSet::new();
        let mut videos = Vec::new();
        for item in data.result {
            if !super::library::valid_bvid(&item.bvid)
                || item.title.len() > 4096
                || item.author.len() > 4096
                || item.pic.len() > 2048
            {
                return Err(BilibiliError::InvalidResponse);
            }
            let title = plain_title(&item.title);
            if title.is_empty() {
                return Err(BilibiliError::InvalidResponse);
            }
            if !seen.insert(item.bvid.clone()) {
                continue;
            }
            let pic = if item.pic.starts_with("//") {
                format!("https:{}", item.pic)
            } else {
                item.pic
            };
            videos.push(VideoSummary {
                bvid: item.bvid,
                title,
                uploader: plain_title(&item.author),
                cover: super::library::safe_artwork(&pic).map(|url| url.to_string()),
                duration: duration(&item.duration).ok_or(BilibiliError::InvalidResponse)?,
            });
        }
        Ok(SearchPage {
            source: self.id.clone(),
            keyword: keyword.into(),
            page,
            pages: data.pages.max(1),
            total: data.total,
            videos,
        })
    }

    pub async fn open_video(&self, value: &str) -> Result<VideoTracks, BilibiliError> {
        let mut input = parse_video_input(value)?.ok_or(BilibiliError::InvalidVideoInput)?;
        let mut seen = HashSet::new();
        for _ in 0..3 {
            let Some(short) = input.short.take() else {
                break;
            };
            if !seen.insert(short.to_string()) {
                return Err(BilibiliError::InvalidVideoInput);
            }
            let target = self.client.short_location(&short).await?;
            input = parse_video_input(target.as_str())?.ok_or(BilibiliError::InvalidVideoInput)?;
        }
        let id = input.id.ok_or(BilibiliError::InvalidVideoInput)?;
        let video = self.video_by_id(&id).await?;
        if video.pages.len() > 1024 {
            return Err(BilibiliError::InvalidResponse);
        }
        let songs = video.into_songs(self).collect::<Vec<_>>();
        let selected = input.part.unwrap_or(0);
        if selected >= songs.len() {
            return Err(BilibiliError::InvalidVideoInput);
        }
        let first = songs.first().ok_or(BilibiliError::InvalidResponse)?;
        let metadata = first
            .bilibili_part
            .as_ref()
            .ok_or(BilibiliError::InvalidResponse)?;
        Ok(VideoTracks {
            source: self.id.clone(),
            title: metadata.video_title.clone(),
            uploader: metadata.uploader.clone(),
            cover: first.cover_art.clone(),
            songs,
            selected,
        })
    }
}
