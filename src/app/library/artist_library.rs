//! Artist browsing, client-side sorting, and incremental display ownership.

use super::helpers::sort_artists;
use super::requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use crate::app::prelude::*;

const VISIBLE_BATCH: usize = 300;

#[derive(Default)]
pub(in crate::app) struct ArtistLibrary {
    items: Vec<Artist>,
    visible: usize,
    sort: ArtistSort,
    load: LibraryLoad,
}

impl ArtistLibrary {
    pub(in crate::app) fn len(&self) -> usize {
        self.items.len()
    }

    pub(in crate::app) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub(in crate::app) fn visible_items(&self) -> &[Artist] {
        &self.items[..self.visible.min(self.items.len())]
    }

    pub(in crate::app) fn loading(&self) -> bool {
        self.load.loading()
    }

    pub(in crate::app) fn sort(&self) -> ArtistSort {
        self.sort
    }

    pub(in crate::app) fn set_sort(&mut self, sort: ArtistSort) -> bool {
        if self.sort == sort {
            return false;
        }
        self.sort = sort;
        sort_artists(&mut self.items, sort);
        self.visible = self.items.len().min(VISIBLE_BATCH);
        true
    }

    pub(in crate::app) fn show_more(&mut self) {
        self.visible = self
            .visible
            .saturating_add(VISIBLE_BATCH)
            .min(self.items.len());
    }

    pub(in crate::app) fn begin(&mut self, requests: &mut LibraryRequests) -> LibraryRequest {
        self.load.begin(requests)
    }

    pub(in crate::app) fn complete(
        &mut self,
        request: LibraryRequest,
        result: Result<Vec<Artist>, String>,
    ) -> Option<Result<&[Artist], &str>> {
        Some(match self.load.complete(request, result)? {
            Ok(mut artists) => {
                // Use the current preference, including changes made while loading.
                sort_artists(&mut artists, self.sort);
                self.visible = artists.len().min(VISIBLE_BATCH);
                self.items = artists;
                Ok(&self.items)
            }
            // Refresh errors preserve both the list and its expanded display.
            Err(error) => Err(error),
        })
    }
}

impl NavidromeApp {
    pub(in crate::app) fn load_artists(&mut self) {
        let Some(api) = self.api.clone() else { return };
        let request = self.state.artists.begin(&mut self.library_requests);
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.artists().await.map_err(error_message);
            let _ = tx.send(Msg::Artists { request, result });
        });
    }

    pub(in crate::app) fn finish_artists(
        &mut self,
        request: LibraryRequest,
        result: Result<Vec<Artist>, String>,
    ) {
        let Some(result) = self.state.artists.complete(request, result) else {
            return;
        };
        match result {
            Ok(artists) => {
                let covers = artists
                    .iter()
                    .filter_map(|artist| artist.cover_art.clone())
                    .take(40)
                    .collect();
                self.preload_covers(covers);
            }
            Err(error) => {
                let error = error.to_string();
                self.notify_error(error);
            }
        }
    }

    pub(in crate::app) fn set_artist_sort(&mut self, sort: ArtistSort, cx: &mut Context<Self>) {
        if !self.state.artists.set_sort(sort) {
            return;
        }
        self.config.artists_sort = sort;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Artist sort save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests;
