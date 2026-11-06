//! Genre cache, list refreshes, and the selected genre's album pages.

use super::album_paging::complete_album_page;
use super::library_helpers::{album_page_offset, ALBUM_PAGE_SIZE};
use super::library_requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use super::paged_collection::PagedCollection;
use super::*;

pub(super) struct GenreDetails {
    name: String,
    albums: PagedCollection<Album>,
}

impl GenreDetails {
    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(super) fn albums(&self) -> &PagedCollection<Album> {
        &self.albums
    }
}

#[derive(Default)]
pub(super) struct GenreLibrary {
    items: Vec<Genre>,
    load: LibraryLoad,
    detail: Option<GenreDetails>,
}

impl GenreLibrary {
    pub(super) fn items(&self) -> &[Genre] {
        &self.items
    }

    pub(super) fn len(&self) -> usize {
        self.items.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub(super) fn loading(&self) -> bool {
        self.load.loading()
    }

    pub(super) fn detail(&self) -> Option<&GenreDetails> {
        self.detail.as_ref()
    }

    /// Restore a server-scoped cache and invalidate list and detail requests.
    pub(super) fn restore(&mut self, cached: Option<Vec<Genre>>) {
        let mut items = cached.unwrap_or_default();
        sort_genres(&mut items);
        *self = Self {
            items,
            ..Self::default()
        };
    }

    pub(super) fn begin_list(&mut self, requests: &mut LibraryRequests) -> LibraryRequest {
        self.load.begin(requests)
    }

    pub(super) fn complete_list(
        &mut self,
        request: LibraryRequest,
        result: Result<Vec<Genre>, String>,
    ) -> Option<Result<&[Genre], &str>> {
        Some(match self.load.complete(request, result)? {
            Ok(mut genres) => {
                sort_genres(&mut genres);
                self.items = genres;
                Ok(&self.items)
            }
            Err(error) => Err(error),
        })
    }

    pub(super) fn select(&mut self, name: String) {
        self.detail = Some(GenreDetails {
            name,
            albums: PagedCollection::default(),
        });
    }

    pub(super) fn begin_albums(
        &mut self,
        requests: &mut PageRequests,
    ) -> Option<(String, PageRequest)> {
        let detail = self.detail.as_mut()?;
        let request = detail.albums.begin(requests)?;
        Some((detail.name.clone(), request))
    }

    pub(super) fn complete_albums(
        &mut self,
        request: PageRequest,
        result: Result<Vec<Album>, String>,
    ) -> Option<Result<Vec<String>, String>> {
        complete_album_page(&mut self.detail.as_mut()?.albums, request, result, None)
    }
}

fn sort_genres(genres: &mut [Genre]) {
    genres.sort_by_key(|genre| genre.value.trim().to_lowercase());
}

impl NavidromeApp {
    pub(super) fn load_genres(&mut self) {
        let Some(api) = self.api.clone() else { return };
        let request = self.state.genres.begin_list(&mut self.library_requests);
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.genres().await.map_err(error_message);
            let _ = tx.send(Msg::Genres { request, result });
        });
    }

    pub(super) fn finish_genres(
        &mut self,
        request: LibraryRequest,
        result: Result<Vec<Genre>, String>,
    ) {
        let Some(result) = self.state.genres.complete_list(request, result) else {
            return;
        };
        match result {
            Ok(genres) => {
                if let Err(error) = self.server_cache_writer.submit(
                    self.config.active_server_id(),
                    config::CacheSnapshot::Genres(genres.to_vec()),
                ) {
                    log::warn!("could not queue genres cache: {error}");
                }
            }
            Err(error) => {
                let error = error.to_string();
                self.notify_error(error);
            }
        }
    }

    pub(super) fn open_genre(&mut self, genre: String) {
        self.state.genres.select(genre);
        self.state.view = View::GenreDetail;
        self.load_more_genre_albums();
    }

    pub(super) fn load_more_genre_albums(&mut self) {
        let Some(api) = self.api.clone() else { return };
        let Some((genre, request)) = self
            .state
            .genres
            .begin_albums(&mut self.album_page_requests)
        else {
            return;
        };
        let offset = album_page_offset(request.page(), ALBUM_PAGE_SIZE);
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api
                .albums_by_genre(&genre, ALBUM_PAGE_SIZE as u32, offset)
                .await
                .map_err(error_message);
            let _ = tx.send(Msg::GenreAlbums { request, result });
        });
    }

    pub(super) fn handle_genre_album_page(
        &mut self,
        request: PageRequest,
        result: Result<Vec<Album>, String>,
    ) {
        let Some(result) = self.state.genres.complete_albums(request, result) else {
            return;
        };
        match result {
            Ok(covers) => self.preload_covers(covers),
            Err(error) => self.notify_error(error),
        }
    }
}

#[cfg(test)]
mod tests;
