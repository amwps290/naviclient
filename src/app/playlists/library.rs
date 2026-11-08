//! Playlist data and request ownership, independent of dialogs and menus.

use super::edit_state::{PlaylistEditCompletion, PlaylistEditOrigin, PlaylistEdits};
use crate::app::library::requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use crate::models::{Playlist, Song};
use crate::playlist::{PlaylistDetails, PlaylistEdited};

const INITIAL_VISIBLE: usize = 50;
const VISIBLE_BATCH: usize = 100;

#[derive(Default)]
pub(in crate::app) struct PlaylistLibrary {
    items: Vec<Playlist>,
    visible: usize,
    selected: Option<PlaylistDetails>,
    list_load: LibraryLoad,
    detail_load: LibraryLoad,
    edits: PlaylistEdits,
    list_task: Option<tokio::task::AbortHandle>,
    detail_task: Option<tokio::task::AbortHandle>,
}

impl Drop for PlaylistLibrary {
    fn drop(&mut self) {
        if let Some(task) = self.list_task.take() {
            task.abort();
        }
        if let Some(task) = self.detail_task.take() {
            task.abort();
        }
    }
}

impl PlaylistLibrary {
    pub(in crate::app) fn items(&self) -> &[Playlist] {
        &self.items
    }

    pub(in crate::app) fn visible_items(&self) -> &[Playlist] {
        &self.items[..self.visible.min(self.items.len())]
    }

    pub(in crate::app) fn show_more(&mut self) {
        self.visible = self
            .visible
            .saturating_add(VISIBLE_BATCH)
            .min(self.items.len());
    }

    pub(in crate::app) fn playlist(&self) -> Option<&Playlist> {
        self.selected.as_ref().map(|details| &details.playlist)
    }

    pub(in crate::app) fn songs(&self) -> &[Song] {
        self.selected.as_ref().map_or(&[], |details| &details.songs)
    }

    pub(in crate::app) fn list_loading(&self) -> bool {
        self.list_load.loading()
    }

    pub(in crate::app) fn list_error(&self) -> Option<&str> {
        self.list_load.error()
    }

    pub(in crate::app) fn detail_loading(&self) -> bool {
        self.detail_load.loading()
    }

    pub(in crate::app) fn editing(&self) -> bool {
        self.edits.editing()
    }

    pub(in crate::app) fn select(&mut self, playlist: Playlist) {
        if let Some(task) = self.detail_task.take() {
            task.abort();
        }
        self.selected = Some(PlaylistDetails {
            playlist,
            songs: Vec::new(),
            unavailable_count: 0,
        });
        self.detail_load = LibraryLoad::default();
    }

    pub(in crate::app) fn begin_list(&mut self, requests: &mut LibraryRequests) -> LibraryRequest {
        if let Some(task) = self.list_task.take() {
            task.abort();
        }
        self.list_load.begin(requests)
    }

    pub(in crate::app) fn attach_list(&mut self, task: tokio::task::AbortHandle) {
        self.list_task = Some(task);
    }
    pub(in crate::app) fn attach_detail(&mut self, task: tokio::task::AbortHandle) {
        self.detail_task = Some(task);
    }

    pub(in crate::app) fn detail_error(&self) -> Option<&str> {
        self.detail_load.error()
    }

    pub(in crate::app) fn complete_list(
        &mut self,
        request: LibraryRequest,
        result: Result<Vec<Playlist>, String>,
    ) -> Option<Result<&[Playlist], &str>> {
        Some(match self.list_load.complete(request, result)? {
            Ok(items) => {
                self.visible = items.len().min(INITIAL_VISIBLE);
                self.items = items;
                Ok(&self.items)
            }
            Err(error) => Err(error),
        })
    }

    pub(in crate::app) fn begin_detail(
        &mut self,
        requests: &mut LibraryRequests,
    ) -> Option<LibraryRequest> {
        self.selected.as_ref()?;
        if let Some(task) = self.detail_task.take() {
            task.abort();
        }
        Some(self.detail_load.begin(requests))
    }

    pub(in crate::app) fn complete_detail(
        &mut self,
        request: LibraryRequest,
        result: Result<PlaylistDetails, String>,
    ) -> Option<Result<&PlaylistDetails, &str>> {
        Some(match self.detail_load.complete(request, result)? {
            Ok(mut details) => {
                // Some servers omit playlist artwork. Preserve track order and
                // use the first available cover for the detail and list views.
                if details.playlist.cover_art.is_none() {
                    details.playlist.cover_art =
                        details.songs.iter().find_map(|song| song.cover_art.clone());
                }
                if let Some(item) = self
                    .items
                    .iter_mut()
                    .find(|item| item.id == details.playlist.id)
                {
                    item.cover_art = details.playlist.cover_art.clone();
                }
                self.selected = Some(details);
                Ok(self
                    .selected
                    .as_ref()
                    .expect("accepted playlist detail exists"))
            }
            Err(error) => Err(error),
        })
    }

    pub(in crate::app) fn begin_edit(
        &mut self,
        requests: &mut LibraryRequests,
        origin: PlaylistEditOrigin,
    ) -> Option<LibraryRequest> {
        let request = self.edits.begin(requests, origin)?;
        self.invalidate_reads();
        Some(request)
    }

    pub(in crate::app) fn complete_edit(
        &mut self,
        request: LibraryRequest,
        result: Result<PlaylistEdited, String>,
    ) -> Option<PlaylistEditCompletion> {
        let completion = self.edits.complete(request, result)?;
        // Reads begun while a write was pending can still contain pre-write
        // data. Invalidate them on success and failure before refreshing.
        self.list_load = LibraryLoad::default();
        self.detail_load = LibraryLoad::default();
        if let Ok(edited) = &completion.result {
            if edited.deleted {
                self.items.retain(|item| item.id != edited.id);
                self.visible = self.visible.min(self.items.len());
                if self
                    .selected
                    .as_ref()
                    .is_some_and(|details| details.playlist.id == edited.id)
                {
                    self.selected = None;
                }
            }
        }
        Some(completion)
    }

    fn invalidate_reads(&mut self) {
        if let Some(task) = self.list_task.take() {
            task.abort();
        }
        if let Some(task) = self.detail_task.take() {
            task.abort();
        }
        self.list_load = LibraryLoad::default();
        self.detail_load = LibraryLoad::default();
    }
}

#[cfg(test)]
mod tests;
