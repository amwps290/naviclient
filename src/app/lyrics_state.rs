//! Current-song lyrics, request identity, and scrolling follow state.

use gpui_kit::Pixels;

use super::library_requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use crate::models::Lyrics;

#[derive(Default)]
pub(super) struct LyricsState {
    song_id: Option<String>,
    lyrics: Option<Lyrics>,
    load: LibraryLoad,
    pub(super) active_index: Option<usize>,
    pub(super) scroll_target: Option<Pixels>,
}

impl LyricsState {
    pub(super) fn lyrics(&self) -> Option<&Lyrics> {
        self.lyrics.as_ref()
    }

    pub(super) fn loading(&self) -> bool {
        self.load.loading()
    }

    pub(super) fn error(&self) -> Option<&str> {
        self.load.error()
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(super) fn reset_follow(&mut self) {
        self.active_index = None;
        self.scroll_target = None;
    }

    /// Keep cached lyrics and coalesce in-flight loads for the same song. A
    /// failed load can be retried; returning to a song gets a new request ID.
    pub(super) fn begin(
        &mut self,
        song_id: &str,
        requests: &mut LibraryRequests,
    ) -> Option<LibraryRequest> {
        self.reset_follow();
        if self.song_id.as_deref() == Some(song_id)
            && (self.lyrics.is_some() || self.load.loading())
        {
            return None;
        }
        self.song_id = Some(song_id.to_string());
        self.lyrics = None;
        Some(self.load.begin(requests))
    }

    pub(super) fn complete(
        &mut self,
        request: LibraryRequest,
        result: Result<Lyrics, String>,
    ) -> bool {
        let Some(result) = self.load.complete(request, result) else {
            return false;
        };
        self.lyrics = result.ok();
        self.reset_follow();
        true
    }
}

#[cfg(test)]
mod tests;
