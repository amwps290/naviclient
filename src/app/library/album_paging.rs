//! Album paging requests and shared page validation and artwork prefetching.

use super::helpers::{album_page_offset, ALBUM_PAGE_SIZE};
use super::paged_collection::PagedCollection;
use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn load_albums(&mut self) {
        self.state.albums.reset();
        self.load_more_albums();
    }

    pub(in crate::app) fn load_more_albums(&mut self) {
        let Some(api) = self.api.clone() else { return };
        let Some(request) = self.state.albums.begin(&mut self.album_page_requests) else {
            return;
        };
        let offset = album_page_offset(request.page(), ALBUM_PAGE_SIZE);
        let sort = self.state.albums_sort;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api
                .albums(ALBUM_PAGE_SIZE as u32, offset, sort)
                .await
                .map_err(error_message);
            let _ = tx.send(Msg::Albums { request, result });
        });
    }

    pub(in crate::app) fn handle_album_page(
        &mut self,
        request: PageRequest,
        result: Result<Vec<Album>, String>,
    ) {
        let handle_start = std::time::Instant::now();
        let Some(result) = complete_album_page(
            &mut self.state.albums,
            request,
            result,
            Some(self.state.albums_sort),
        ) else {
            return;
        };
        match result {
            Ok(covers) => {
                let cover_count = covers.len();
                self.preload_covers(covers);
                log::debug!(
                    "albums page handled in {:?}; total={} covers_queued={cover_count}",
                    handle_start.elapsed(),
                    self.state.albums.len(),
                );
            }
            Err(error) => self.notify_error(error),
        }
    }
}

/// Stale responses produce neither artwork requests nor error notifications.
pub(in crate::app) fn complete_album_page(
    collection: &mut PagedCollection<Album>,
    request: PageRequest,
    result: Result<Vec<Album>, String>,
    sort: Option<AlbumSort>,
) -> Option<Result<Vec<String>, String>> {
    if !collection.accepts(request) {
        return None;
    }
    let result = result.map(|mut albums| {
        // 服务端已按所选方式排序；仅“最近添加”需要同页内修正时间顺序。
        if sort == Some(AlbumSort::NewestAdded) {
            albums.sort_by_key(|album| album.created.clone().unwrap_or_default());
            albums.reverse();
        }
        albums
    });
    collection.complete(request, result).map(|result| {
        result.map(|albums| {
            albums
                .iter()
                .filter_map(|album| album.cover_art.clone())
                .take(80)
                .collect()
        })
    })
}

#[cfg(test)]
mod tests;
