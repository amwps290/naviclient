//! Lazy folder artwork metadata. Only current list snapshots may publish covers.

use crate::app::prelude::*;
use crate::source::{bilibili::client::BilibiliError, ProviderKind, SourceId};

impl NavidromeApp {
    pub(in crate::app) fn poll_bilibili_folder_covers(&mut self) -> bool {
        let snapshot = self
            .state
            .playlists
            .list_snapshot()
            .filter(|_| self.browsing_bilibili());
        self.state.bilibili.sync_cover_snapshot(snapshot);
        let Some(snapshot) = snapshot else {
            return false;
        };
        let wanted = if self.state.view == View::Playlists {
            super::state::matching_folders(
                self.state.playlists.items(),
                &self.state.bilibili.folder_query,
            )
            .into_iter()
            .take(self.state.bilibili.folder_limit.max(50))
            .map(|folder| folder.id.clone())
            .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        self.state
            .bilibili
            .prune_cover_jobs(&wanted.iter().cloned().collect::<HashSet<_>>());
        if !self.state.bilibili.cover_query_settled() {
            return false;
        }
        let Some(api) = self
            .api
            .clone()
            .filter(|api| api.id().provider == ProviderKind::Bilibili)
        else {
            return false;
        };
        let source = api.id().clone();
        let mut changed = false;
        for id in wanted {
            if !self.state.bilibili.needs_cover(&id) {
                continue;
            }
            let Ok(numeric) = id.parse::<u64>() else {
                continue;
            };
            let request = self.library_requests.next();
            let api = api.clone();
            let source = source.clone();
            let folder_id = id.clone();
            let slots = self.folder_cover_slots.clone();
            let stop_signal = self.state.bilibili.cover_stop_signal();
            let tx = self.tx.clone();
            let task = self.runtime.spawn(async move {
                let Ok(_permit) = slots.acquire_owned().await else {
                    return;
                };
                if stop_signal.load(Ordering::Relaxed) {
                    return;
                }
                let result = tokio::time::timeout(
                    Duration::from_secs(20),
                    api.bilibili_folder_cover(numeric),
                )
                .await
                .unwrap_or(Err(BilibiliError::Timeout));
                if result
                    .as_ref()
                    .is_err_and(|error| stop_cover_requests(*error))
                {
                    // Stop queued requests before releasing this semaphore
                    // permit, without waiting for the next UI poll.
                    stop_signal.store(true, Ordering::Relaxed);
                }
                let _ = tx.send(Msg::BilibiliFolderCover {
                    source,
                    snapshot,
                    request,
                    folder_id,
                    result,
                });
            });
            self.state
                .bilibili
                .attach_cover(id, request, task.abort_handle());
            changed = true;
        }
        changed
    }

    pub(in crate::app) fn finish_bilibili_folder_cover(
        &mut self,
        source: SourceId,
        snapshot: LibraryRequest,
        request: LibraryRequest,
        id: String,
        result: Result<Option<String>, BilibiliError>,
    ) {
        if self.api.as_ref().map(|api| api.id()) != Some(&source)
            || self.state.playlists.list_snapshot() != Some(snapshot)
            || !self.state.bilibili.finish_cover(&id, request)
        {
            return;
        }
        match result {
            Ok(cover) => {
                if self
                    .state
                    .playlists
                    .apply_folder_cover(snapshot, &id, cover.clone())
                {
                    self.ensure_cover(cover.as_deref(), false);
                }
            }
            Err(error) => {
                self.state.bilibili.cover_failed(stop_cover_requests(error));
                log::warn!("Bilibili folder artwork metadata unavailable: {error}");
            }
        }
    }
}

fn stop_cover_requests(error: BilibiliError) -> bool {
    matches!(
        error,
        BilibiliError::RateLimited
            | BilibiliError::NotAuthenticated
            | BilibiliError::AccountMismatch
            | BilibiliError::Http(401)
    )
}
