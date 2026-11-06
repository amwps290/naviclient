//! Playlist data and request ownership, independent of dialogs and menus.

use super::library_requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use super::playlist_edit_state::{PlaylistEditCompletion, PlaylistEditOrigin, PlaylistEdits};
use crate::models::{Playlist, Song};
use crate::playlist::{PlaylistDetails, PlaylistEdited};

const INITIAL_VISIBLE: usize = 50;
const VISIBLE_BATCH: usize = 100;

#[derive(Default)]
pub(super) struct PlaylistLibrary {
    items: Vec<Playlist>,
    visible: usize,
    selected: Option<PlaylistDetails>,
    list_load: LibraryLoad,
    detail_load: LibraryLoad,
    edits: PlaylistEdits,
}

impl PlaylistLibrary {
    pub(super) fn items(&self) -> &[Playlist] {
        &self.items
    }

    pub(super) fn visible_items(&self) -> &[Playlist] {
        &self.items[..self.visible.min(self.items.len())]
    }

    pub(super) fn show_more(&mut self) {
        self.visible = self
            .visible
            .saturating_add(VISIBLE_BATCH)
            .min(self.items.len());
    }

    pub(super) fn playlist(&self) -> Option<&Playlist> {
        self.selected.as_ref().map(|details| &details.playlist)
    }

    pub(super) fn songs(&self) -> &[Song] {
        self.selected.as_ref().map_or(&[], |details| &details.songs)
    }

    pub(super) fn list_loading(&self) -> bool {
        self.list_load.loading()
    }

    pub(super) fn detail_loading(&self) -> bool {
        self.detail_load.loading()
    }

    pub(super) fn editing(&self) -> bool {
        self.edits.editing()
    }

    pub(super) fn select(&mut self, playlist: Playlist) {
        self.selected = Some(PlaylistDetails {
            playlist,
            songs: Vec::new(),
        });
        self.detail_load = LibraryLoad::default();
    }

    pub(super) fn begin_list(&mut self, requests: &mut LibraryRequests) -> LibraryRequest {
        self.list_load.begin(requests)
    }

    pub(super) fn complete_list(
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

    pub(super) fn begin_detail(
        &mut self,
        requests: &mut LibraryRequests,
    ) -> Option<LibraryRequest> {
        self.selected.as_ref()?;
        Some(self.detail_load.begin(requests))
    }

    pub(super) fn complete_detail(
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

    pub(super) fn begin_edit(
        &mut self,
        requests: &mut LibraryRequests,
        origin: PlaylistEditOrigin,
    ) -> Option<LibraryRequest> {
        let request = self.edits.begin(requests, origin)?;
        self.invalidate_reads();
        Some(request)
    }

    pub(super) fn complete_edit(
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
        self.list_load = LibraryLoad::default();
        self.detail_load = LibraryLoad::default();
    }
}

#[cfg(test)]
mod tests;
