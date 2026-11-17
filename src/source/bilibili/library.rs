//! Ordered, bounded own-favorites traversal. A partial load is never "play all".

use super::{
    client::{BilibiliClient, BilibiliError, Envelope},
    BilibiliProvider,
};
use crate::models::Playlist;
use crate::models::{BilibiliPart, Song};
use crate::playlist::loading::{playlist_update_channel, PlaylistProgress, PlaylistUpdate};
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::time::Instant;
use tokio::sync::mpsc::Sender;

mod batches;
use batches::TrackUpdates;

#[cfg(test)]
mod tests;

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

pub(crate) struct FolderTrackSummary {
    pub folder: FavoriteFolder,
    pub progress: PlaylistProgress,
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
        let (sender, mut receiver) = playlist_update_channel();
        let load = async {
            let result = self.load_favorite_tracks(folder_id, &sender).await;
            drop(sender);
            result
        };
        let collect = async {
            let mut songs = Vec::new();
            while let Some(update) = receiver.recv().await {
                if let PlaylistUpdate::Batch { songs: batch, .. } = update {
                    songs.extend(batch);
                }
            }
            songs
        };
        // Dropping this future also drops the traversal and its JoinSet. A
        // failed traversal discards the collected preview instead of returning
        // a partial result through the complete-details API.
        let (summary, songs) = tokio::join!(load, collect);
        let summary = summary?;
        Ok(FolderTracks {
            folder: summary.folder,
            songs,
            pages: summary.progress.pages,
            videos: summary.progress.videos,
            unavailable: summary.progress.unavailable_count,
        })
    }

    pub(crate) async fn load_favorite_tracks(
        &self,
        folder_id: u64,
        sender: &Sender<PlaylistUpdate>,
    ) -> Result<FolderTrackSummary, BilibiliError> {
        let started = Instant::now();
        let mut updates = TrackUpdates::new(sender, started);
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
        let mut seen = HashSet::new();
        let mut page_signatures = HashSet::new();
        let mut folder = None;
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
            let current_folder = response.info.folder;
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
            let mut bvids = Vec::new();
            for video in medias {
                if video.kind != 2 || !valid_bvid(&video.bvid) {
                    updates.progress.unavailable_count += 1;
                } else if seen.insert(video.bvid.clone()) {
                    bvids.push(video.bvid);
                }
            }
            updates.progress.pages = page;
            updates.progress.videos = seen.len();
            updates
                .send(PlaylistUpdate::Metadata(Playlist {
                    id: current_folder.id.to_string(),
                    name: current_folder.title.clone(),
                    ..Default::default()
                }))
                .await?;
            folder = Some(current_folder);
            if let Err(error) = self.load_page_videos(bvids, &mut updates).await {
                // A quick later failure must not discard the valid prefix still
                // waiting for the batching timer.
                updates.flush().await?;
                return Err(error);
            }
            updates.report_progress().await?;
            if !response.has_more {
                break;
            }
            if page == MAX_PAGES {
                return Err(BilibiliError::LimitExceeded);
            }
        }
        log::debug!(
            "bilibili favorites loaded; elapsed_ms={} pages={} videos={} songs={} unavailable={}",
            started.elapsed().as_millis(),
            updates.progress.pages,
            updates.progress.videos,
            updates.progress.songs,
            updates.progress.unavailable_count,
        );
        Ok(FolderTrackSummary {
            folder: folder.ok_or(BilibiliError::InvalidResponse)?,
            progress: updates.progress,
        })
    }

    async fn load_page_videos(
        &self,
        bvids: Vec<String>,
        updates: &mut TrackUpdates<'_>,
    ) -> Result<(), BilibiliError> {
        let mut jobs = tokio::task::JoinSet::new();
        let mut ready: BTreeMap<usize, Result<Video, BilibiliError>> = BTreeMap::new();
        let mut next_start = 0;
        let mut next_publish = 0;
        while next_publish < bvids.len() {
            // Completed results waiting behind a slow predecessor still occupy
            // a slot. Both in-flight work and ordered lookahead stay bounded.
            while next_start < bvids.len() && jobs.len() + ready.len() < DETAIL_CONCURRENCY {
                let provider = self.clone();
                let index = next_start;
                let bvid = bvids[index].clone();
                jobs.spawn(async move { (index, provider.video(&bvid).await) });
                next_start += 1;
            }
            if let Some(result) = ready.remove(&next_publish) {
                match result {
                    Ok(video) => {
                        if updates.progress.songs.saturating_add(video.pages.len()) > MAX_PARTS {
                            return Err(BilibiliError::LimitExceeded);
                        }
                        for song in video.into_songs(self) {
                            updates.push(song).await?;
                        }
                        updates.flush_first().await?;
                    }
                    Err(
                        BilibiliError::Unavailable
                        | BilibiliError::PermissionDenied
                        | BilibiliError::RegionRestricted,
                    ) => updates.progress.unavailable_count += 1,
                    Err(error) => return Err(error),
                }
                next_publish += 1;
                continue;
            }
            let result = if let Some(deadline) = updates.deadline() {
                tokio::select! {
                    result = jobs.join_next() => result,
                    _ = tokio::time::sleep_until(deadline) => {
                        updates.flush().await?;
                        continue;
                    }
                }
            } else {
                jobs.join_next().await
            };
            let (index, result) = result
                .ok_or(BilibiliError::InvalidResponse)?
                .map_err(|_| BilibiliError::Network)?;
            ready.insert(index, result);
        }
        Ok(())
    }
}

impl Video {
    fn into_songs(self, provider: &BilibiliProvider) -> impl Iterator<Item = Song> {
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
        let source = provider.id.clone();
        self.pages.into_iter().map(move |part| {
            let mut song = Song {
                source: Some(source.clone()),
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
