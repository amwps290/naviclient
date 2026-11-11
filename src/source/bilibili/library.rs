//! Ordered, bounded own-favorites traversal. A partial load is never "play all".

use super::{
    client::{BilibiliClient, BilibiliError, Envelope},
    BilibiliProvider,
};
use crate::models::{BilibiliPart, Song};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

const PAGE_SIZE: usize = 20;
const MAX_PAGES: usize = 1000;
const MAX_PARTS: usize = 100_000;
const DETAIL_CONCURRENCY: usize = 4;

#[derive(Clone, Debug, Deserialize)]
pub struct FavoriteFolder {
    pub id: u64,
    pub title: String,
    #[serde(default)]
    pub media_count: u32,
}

#[derive(Clone, Debug)]
pub struct FolderTracks {
    pub folder: FavoriteFolder,
    pub songs: Vec<Song>,
    pub pages: usize,
    pub videos: usize,
    /// Deleted/unsupported entries are skipped; transient failures abort.
    pub unavailable: usize,
}

#[derive(Deserialize)]
struct Folders {
    #[serde(default)]
    list: Option<Vec<FavoriteFolder>>,
}
#[derive(Deserialize)]
struct Owner {
    mid: u64,
    #[serde(default)]
    name: String,
}
#[derive(Deserialize)]
struct FolderInfo {
    #[serde(flatten)]
    folder: FavoriteFolder,
    upper: Owner,
}
#[derive(Deserialize)]
struct FolderPage {
    info: FolderInfo,
    #[serde(default)]
    medias: Option<Vec<FavoriteVideo>>,
    has_more: bool,
}
#[derive(Deserialize)]
struct FavoriteVideo {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    bvid: String,
    #[serde(rename = "type", default)]
    kind: u32,
}
#[derive(Deserialize)]
pub(super) struct Video {
    pub bvid: String,
    pub title: String,
    #[serde(default)]
    pic: String,
    owner: Owner,
    pub pages: Vec<Part>,
    #[serde(default)]
    ugc_season: Option<Collection>,
}

#[derive(Deserialize)]
struct Collection {
    #[serde(default)]
    title: String,
    #[serde(default)]
    sections: Vec<CollectionSection>,
}

#[derive(Deserialize)]
struct CollectionSection {
    #[serde(default)]
    episodes: Vec<CollectionEpisode>,
}

#[derive(Deserialize)]
struct CollectionEpisode {
    #[serde(default)]
    bvid: String,
    #[serde(default)]
    title: String,
}
#[derive(Deserialize)]
pub(super) struct Part {
    pub cid: u64,
    pub page: u32,
    pub part: String,
    pub duration: u64,
}

pub(super) fn valid_bvid(bvid: &str) -> bool {
    bvid.len() == 12 && bvid.starts_with("BV") && bvid.bytes().all(|b| b.is_ascii_alphanumeric())
}

pub(super) fn parse_media_id(id: &str) -> Result<(&str, u64), BilibiliError> {
    let (bvid, cid) = id.split_once(':').ok_or(BilibiliError::InvalidResponse)?;
    let numeric = cid
        .parse::<u64>()
        .map_err(|_| BilibiliError::InvalidResponse)?;
    if !valid_bvid(bvid) || numeric == 0 || numeric.to_string() != cid {
        return Err(BilibiliError::InvalidResponse);
    }
    Ok((bvid, numeric))
}

impl BilibiliProvider {
    pub async fn favorite_folders(&self) -> Result<Vec<FavoriteFolder>, BilibiliError> {
        let account = self.session.account_id().to_string();
        let response = self
            .client
            .get(
                false,
                "/x/v3/fav/folder/created/list-all",
                &[("up_mid", &account)],
                Some(&self.session),
            )
            .await?;
        let folders = BilibiliClient::json::<Envelope<Folders>>(response)
            .await?
            .into_data()?
            .list
            .unwrap_or_default();
        let mut seen = HashSet::new();
        if folders
            .iter()
            .any(|folder| folder.id == 0 || !seen.insert(folder.id))
        {
            return Err(BilibiliError::InvalidResponse);
        }
        Ok(folders)
    }

    pub(super) async fn video(&self, bvid: &str) -> Result<Video, BilibiliError> {
        if !valid_bvid(bvid) {
            return Err(BilibiliError::InvalidResponse);
        }
        let response = self
            .client
            .get(
                false,
                "/x/web-interface/view",
                &[("bvid", bvid)],
                Some(&self.session),
            )
            .await?;
        let mut video = BilibiliClient::json::<Envelope<Video>>(response)
            .await?
            .into_data()?;
        let mut ids = HashSet::new();
        if video.bvid != bvid
            || video.pages.is_empty()
            || video.pages.len() > MAX_PARTS
            || video.pages.iter().any(|part| {
                part.cid == 0
                    || part.page == 0
                    || !ids.insert(part.cid)
                    || part.duration > i32::MAX as u64
            })
        {
            return Err(BilibiliError::InvalidResponse);
        }
        video.pages.sort_by_key(|part| part.page);
        if video
            .pages
            .iter()
            .enumerate()
            .any(|(i, part)| part.page as usize != i + 1)
        {
            return Err(BilibiliError::InvalidResponse);
        }
        Ok(video)
    }

    pub async fn favorite_tracks(&self, folder_id: u64) -> Result<FolderTracks, BilibiliError> {
        // Only the authenticated account's own folders may be opened, including
        // restored/stale playlist IDs. Never substitute a different owner.
        if !self
            .favorite_folders()
            .await?
            .iter()
            .any(|folder| folder.id == folder_id)
        {
            return Err(BilibiliError::PermissionDenied);
        }
        let mut bvids = Vec::new();
        let mut seen = HashSet::new();
        let mut unavailable = 0;
        let mut page_signatures = HashSet::new();
        let mut folder = None;
        let mut page_count = 0;
        let id = folder_id.to_string();
        for page in 1..=MAX_PAGES {
            let pn = page.to_string();
            let response = self
                .client
                .get(
                    false,
                    "/x/v3/fav/resource/list",
                    &[
                        ("media_id", &id),
                        ("pn", &pn),
                        ("ps", "20"),
                        ("order", "mtime"),
                        ("type", "0"),
                    ],
                    Some(&self.session),
                )
                .await?;
            let response = BilibiliClient::json::<Envelope<FolderPage>>(response)
                .await?
                .into_data()?;
            if response.info.folder.id != folder_id
                || response.info.upper.mid != self.session.account_id()
            {
                return Err(BilibiliError::AccountMismatch);
            }
            if response.info.folder.media_count as usize > MAX_PAGES * PAGE_SIZE {
                return Err(BilibiliError::LimitExceeded);
            }
            folder = Some(response.info.folder);
            page_count = page;
            let medias = response.medias.unwrap_or_default();
            if medias.len() > PAGE_SIZE || (response.has_more && medias.is_empty()) {
                return Err(BilibiliError::InvalidResponse);
            }
            if response.has_more
                && !page_signatures.insert(
                    medias
                        .iter()
                        .map(|video| (video.id, video.bvid.clone(), video.kind))
                        .collect::<Vec<_>>(),
                )
            {
                return Err(BilibiliError::InvalidResponse);
            }
            for video in medias {
                if video.kind != 2 || !valid_bvid(&video.bvid) {
                    unavailable += 1;
                } else if seen.insert(video.bvid.clone()) {
                    bvids.push(video.bvid);
                }
            }
            if !response.has_more {
                break;
            }
            if page == MAX_PAGES {
                return Err(BilibiliError::LimitExceeded);
            }
        }
        let videos = bvids.len();
        let mut songs = Vec::new();
        // JoinSet aborts outstanding work on cancellation; retain API order
        // independently of response completion order.
        for chunk in bvids.chunks(DETAIL_CONCURRENCY) {
            let mut jobs = tokio::task::JoinSet::new();
            for (index, bvid) in chunk.iter().enumerate() {
                let provider = self.clone();
                let bvid = bvid.clone();
                jobs.spawn(async move { (index, provider.video(&bvid).await) });
            }
            let mut results = HashMap::new();
            while let Some(result) = jobs.join_next().await {
                let (index, result) = result.map_err(|_| BilibiliError::Network)?;
                results.insert(index, result);
            }
            for index in 0..chunk.len() {
                match results
                    .remove(&index)
                    .ok_or(BilibiliError::InvalidResponse)?
                {
                    Ok(video) => {
                        if songs.len().saturating_add(video.pages.len()) > MAX_PARTS {
                            return Err(BilibiliError::LimitExceeded);
                        }
                        songs.extend(video.into_songs(self));
                    }
                    Err(
                        BilibiliError::Unavailable
                        | BilibiliError::PermissionDenied
                        | BilibiliError::RegionRestricted,
                    ) => unavailable += 1,
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(FolderTracks {
            folder: folder.ok_or(BilibiliError::InvalidResponse)?,
            songs,
            pages: page_count,
            videos,
            unavailable,
        })
    }
}

impl Video {
    fn into_songs(self, provider: &BilibiliProvider) -> Vec<Song> {
        let count = self.pages.len() as u32;
        let collection_title = self
            .ugc_season
            .as_ref()
            .map(|collection| collection.title.clone())
            .unwrap_or_default();
        let title = self
            .ugc_season
            .as_ref()
            .filter(|_| count == 1)
            .and_then(|collection| {
                collection
                    .sections
                    .iter()
                    .flat_map(|section| &section.episodes)
                    .find(|episode| episode.bvid == self.bvid && !episode.title.trim().is_empty())
            })
            .map(|episode| episode.title.clone())
            .unwrap_or_else(|| self.title.clone());
        self.pages
            .into_iter()
            .map(|part| {
                let mut song = Song {
                    source: Some(provider.id.clone()),
                    id: format!("{}:{}", self.bvid, part.cid),
                    title: title.clone(),
                    duration: Some(part.duration as i32),
                    bilibili_part: Some(BilibiliPart {
                        bvid: self.bvid.clone(),
                        cid: part.cid,
                        index: part.page,
                        count,
                        title: part.part,
                        uploader: self.owner.name.clone(),
                        video_title: self.title.clone(),
                        collection_title: collection_title.clone(),
                    }),
                    cover_art: safe_artwork(&self.pic).map(|url| url.to_string()),
                    ..Default::default()
                };
                song.normalize_bilibili_title();
                song
            })
            .collect()
    }
}

/// Public artwork is allowed only on Bilibili's image CDN; never send cookies.
pub(super) fn safe_artwork(value: &str) -> Option<url::Url> {
    let mut url = url::Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.host_str()?.ends_with(".hdslb.com")
    {
        return None;
    }
    url.set_scheme("https").ok()?;
    Some(url)
}
