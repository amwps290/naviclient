//! Current-song lyrics, request identity, and scrolling follow state.

use gpui_kit::Pixels;

use crate::app::library::requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use crate::lyrics::{
    service::{LoadOutcome, LyricsStatus},
    Document,
};
use crate::models::Lyrics;
use crate::source::MediaKey;

#[derive(Default)]
pub(in crate::app) struct LyricsState {
    media: Option<MediaKey>,
    lyrics: Option<Lyrics>,
    load: LibraryLoad,
    pub(in crate::app) document: Option<Document>,
    pub(in crate::app) status: LyricsStatus,
    pub(in crate::app) warning: Option<LyricsStatus>,
    pub(in crate::app) task: Option<tokio::task::JoinHandle<()>>,
    pub(in crate::app) active_index: Option<usize>,
    pub(in crate::app) scroll_target: Option<Pixels>,
}

impl LyricsState {
    pub(in crate::app) fn media(&self) -> Option<&MediaKey> {
        self.media.as_ref()
    }
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
        media: &MediaKey,
        requests: &mut LibraryRequests,
    ) -> Option<LibraryRequest> {
        self.reset_follow();
        if self.media.as_ref() == Some(media) && (self.lyrics.is_some() || self.load.loading()) {
            return None;
        }
        self.media = Some(media.clone());
        self.lyrics = None;
        self.document = None;
        self.warning = None;
        self.status = LyricsStatus::ReadingSource;
        self.cancel_task();
        Some(self.load.begin(requests))
    }

    pub(in crate::app) fn refresh(
        &mut self,
        media: &MediaKey,
        requests: &mut LibraryRequests,
    ) -> LibraryRequest {
        self.cancel_task();
        if self.media.as_ref() != Some(media) {
            self.lyrics = None;
            self.document = None;
        }
        self.media = Some(media.clone());
        self.warning = None;
        self.status = LyricsStatus::ReadingSource;
        self.load.begin(requests)
    }

    pub(in crate::app) fn progress(&mut self, request: LibraryRequest, status: LyricsStatus) {
        if self.load.pending(request) {
            self.status = status;
        }
    }

    pub(in crate::app) fn finish(&mut self, request: LibraryRequest, outcome: LoadOutcome) -> bool {
        // Failed refreshes keep the current document visible with a warning.
        let result = if outcome.document.is_none() && self.document.is_none() {
            outcome.error.map_or(Ok(()), Err)
        } else {
            Ok(())
        };
        if self.load.complete(request, result).is_none() {
            return false;
        }
        self.warning = outcome.warning;
        if let Some(document) = outcome.document {
            self.lyrics = Some(document.display_lyrics());
            self.document = Some(document);
            self.status = outcome.status;
        } else if self.document.is_some() {
            self.warning = Some(outcome.status);
            self.status = if self.document.as_ref().is_some_and(|d| d.instrumental) {
                LyricsStatus::Instrumental
            } else {
                LyricsStatus::Ready
            };
        } else {
            self.status = outcome.status;
        }
        self.reset_follow();
        true
    }

    pub(in crate::app) fn apply(&mut self, document: Document) {
        self.cancel_task();
        self.load = Default::default();
        self.status = if document.instrumental {
            LyricsStatus::Instrumental
        } else {
            LyricsStatus::Ready
        };
        self.warning = None;
        self.lyrics = Some(document.display_lyrics());
        self.document = Some(document);
        self.reset_follow();
    }

    pub(in crate::app) fn cancel_task(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }

    #[cfg(test)]
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

impl Drop for LyricsState {
    fn drop(&mut self) {
        self.cancel_task();
    }
}

#[cfg(test)]
mod tests;
