//! Current-song lyrics, request identity, and scrolling follow state.

use gpui_kit::Pixels;

use crate::app::library::requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use crate::models::Lyrics;

#[derive(Default)]
pub(in crate::app) struct LyricsState {
    song_id: Option<String>,
    lyrics: Option<Lyrics>,
    load: LibraryLoad,
    pub(in crate::app) active_index: Option<usize>,
    pub(in crate::app) scroll_target: Option<Pixels>,
}

impl LyricsState {
    pub(in crate::app) fn lyrics(&self) -> Option<&Lyrics> {
        self.lyrics.as_ref()
    }

    pub(in crate::app) fn loading(&self) -> bool {
        self.load.loading()
    }

    pub(in crate::app) fn error(&self) -> Option<&str> {
        self.load.error()
    }

    pub(in crate::app) fn clear(&mut self) {
        *self = Self::default();
    }

    pub(in crate::app) fn reset_follow(&mut self) {
        self.active_index = None;
        self.scroll_target = None;
    }

    /// Keep cached lyrics and coalesce in-flight loads for the same song. A
    /// failed load can be retried; returning to a song gets a new request ID.
    pub(in crate::app) fn begin(
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

    pub(in crate::app) fn complete(
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
