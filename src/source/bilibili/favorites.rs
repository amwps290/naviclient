//! Video-level favorite changes with owned-folder checks and verified outcomes.

use super::{
    client::{BilibiliClient, BilibiliError, Envelope},
    BilibiliProvider,
};
use crate::source::SourceId;
use serde::Deserialize;
use std::{collections::HashSet, time::Duration};

#[derive(Clone, Debug)]
pub struct FavoriteVideo {
    pub bvid: String,
    pub aid: u64,
    pub title: String,
}
#[derive(Clone, Debug)]
pub struct FavoriteFolderChoice {
    pub id: u64,
    pub title: String,
    pub contains: Option<bool>,
}
#[derive(Clone, Debug)]
pub struct FavoriteSnapshot {
    pub source: SourceId,
    pub account_id: u64,
    pub video: FavoriteVideo,
    pub folders: Vec<FavoriteFolderChoice>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FavoriteOperation {
    pub folder_id: u64,
    pub present: bool,
}
#[derive(Clone, Debug)]
pub enum FavoriteOutcome {
    Verified(FavoriteSnapshot),
    Uncertain {
        snapshot: FavoriteSnapshot,
        error: BilibiliError,
    },
}
impl FavoriteSnapshot {
    pub fn membership(&self, folder: u64) -> Option<bool> {
        self.folders
            .iter()
            .find(|candidate| candidate.id == folder)
            .and_then(|folder| folder.contains)
    }
}

#[derive(Deserialize)]
struct MembershipData {
    #[serde(default)]
    list: Option<Vec<MembershipFolder>>,
}
#[derive(Deserialize)]
struct MembershipFolder {
    id: u64,
    title: String,
    #[serde(default)]
    mid: Option<u64>,
    #[serde(default)]
    fav_state: Option<u32>,
}

impl BilibiliProvider {
    async fn favorite_memberships(
        &self,
        video: FavoriteVideo,
    ) -> Result<FavoriteSnapshot, BilibiliError> {
        let account = self.session.account_id().to_string();
        let aid = video.aid.to_string();
        let response = self
            .client
            .get(
                false,
                "/x/v3/fav/folder/created/list-all",
                &[("up_mid", &account), ("rid", &aid), ("type", "2")],
                Some(&self.session),
            )
            .await?;
        let data = BilibiliClient::json::<Envelope<MembershipData>>(response)
            .await?
            .into_data()?;
        let mut seen = HashSet::new();
        let mut folders = Vec::new();
        for folder in data.list.unwrap_or_default() {
            if folder.id == 0
                || !seen.insert(folder.id)
                || folder.title.len() > 4096
                || folder
                    .mid
                    .is_some_and(|mid| mid != self.session.account_id())
            {
                return Err(BilibiliError::PermissionDenied);
            }
            let contains = match folder.fav_state {
                Some(0) => Some(false),
                Some(1) => Some(true),
                None => None,
                _ => return Err(BilibiliError::InvalidResponse),
            };
            folders.push(FavoriteFolderChoice {
                id: folder.id,
                title: folder.title,
                contains,
            });
        }
        Ok(FavoriteSnapshot {
            source: self.id.clone(),
            account_id: self.session.account_id(),
            video,
            folders,
        })
    }

    pub async fn prepare_video_favorite(
        &self,
        bvid: &str,
    ) -> Result<FavoriteSnapshot, BilibiliError> {
        if !super::library::valid_bvid(bvid) {
            return Err(BilibiliError::InvalidVideoInput);
        }
        self.verify().await?;
        let video = self.video(bvid).await?;
        if video.aid == 0 {
            return Err(BilibiliError::InvalidResponse);
        }
        self.favorite_memberships(FavoriteVideo {
            bvid: video.bvid,
            aid: video.aid,
            title: video.title,
        })
        .await
    }

    pub async fn change_video_favorite(
        &self,
        snapshot: &FavoriteSnapshot,
        operation: FavoriteOperation,
    ) -> Result<FavoriteOutcome, BilibiliError> {
        if snapshot.source != self.id
            || snapshot.account_id != self.session.account_id()
            || operation.folder_id == 0
        {
            return Err(BilibiliError::AccountMismatch);
        }
        // This timeout only surrounds GET preflight. No write has begun yet.
        let (current, membership) = tokio::time::timeout(Duration::from_secs(30), async {
            let current = self.prepare_video_favorite(&snapshot.video.bvid).await?;
            if current.video.aid != snapshot.video.aid {
                return Err(BilibiliError::InvalidResponse);
            }
            let membership = current
                .membership(operation.folder_id)
                .ok_or(BilibiliError::PermissionDenied)?;
            if membership != operation.present {
                // Independently verify the target's upper.mid before a write;
                // do not trust a UI-supplied ID or an absent list-row owner.
                self.favorite_folder_cover(operation.folder_id).await?;
            }
            Ok::<_, BilibiliError>((current, membership))
        })
        .await
        .map_err(|_| BilibiliError::Timeout)??;
        if membership == operation.present {
            return Ok(FavoriteOutcome::Verified(current));
        }
        let aid = current.video.aid.to_string();
        let folder = operation.folder_id.to_string();
        let csrf = self.session.csrf()?;
        let response = self
            .client
            .post_favorite(
                &[
                    ("rid", &aid),
                    ("type", "2"),
                    (
                        "add_media_ids",
                        if operation.present { &folder } else { "" },
                    ),
                    (
                        "del_media_ids",
                        if operation.present { "" } else { &folder },
                    ),
                    ("csrf", &csrf),
                ],
                &self.session,
            )
            .await;
        let acknowledged = match response {
            Ok(response) => BilibiliClient::json::<Envelope<serde_json::Value>>(response)
                .await
                .and_then(|result| result.into_success()),
            Err(error) => Err(error),
        };
        // Never resend a POST, including when its response was lost. A confirmed
        // desired membership is success; otherwise the caller must check state.
        if matches!(
            acknowledged,
            Err(BilibiliError::RateLimited
                | BilibiliError::NotAuthenticated
                | BilibiliError::AccountMismatch)
        ) {
            return Ok(uncertain(current, operation, acknowledged.unwrap_err()));
        }
        let checked = tokio::time::timeout(
            Duration::from_secs(20),
            self.favorite_memberships(current.video.clone()),
        )
        .await;
        if let Ok(Ok(checked)) = checked {
            if checked.membership(operation.folder_id) == Some(operation.present) {
                return Ok(FavoriteOutcome::Verified(checked));
            }
            return Ok(uncertain(
                checked,
                operation,
                acknowledged.err().unwrap_or(BilibiliError::InvalidResponse),
            ));
        }
        Ok(uncertain(
            current,
            operation,
            acknowledged.err().unwrap_or(BilibiliError::Network),
        ))
    }
}

fn uncertain(
    mut snapshot: FavoriteSnapshot,
    operation: FavoriteOperation,
    error: BilibiliError,
) -> FavoriteOutcome {
    if let Some(folder) = snapshot
        .folders
        .iter_mut()
        .find(|folder| folder.id == operation.folder_id)
    {
        folder.contains = None;
    }
    FavoriteOutcome::Uncertain { snapshot, error }
}

#[cfg(test)]
mod tests;
