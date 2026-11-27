//! Current-song lyrics, request identity, and scrolling follow state.

use std::time::{Duration, Instant};

use gpui_kit::Pixels;

pub(in crate::app) const MANUAL_SCROLL_PAUSE: Duration = Duration::from_secs(4);

use crate::app::library::requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use crate::lyrics::{
    cache::LyricsCache,
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
    pub(in crate::app) follow_reset: bool,
    pub(in crate::app) follow_resume: bool,
    pub(in crate::app) manual_scroll_until: Option<Instant>,
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
        self.reset_follow();
    }

    pub(in crate::app) fn reset_follow(&mut self) {
        self.active_index = None;
        self.scroll_target = None;
        self.follow_reset = true;
        self.follow_resume = false;
        self.manual_scroll_until = None;
    }

    pub(in crate::app) fn pause_follow_for_scroll(&mut self, now: Instant) {
        self.scroll_target = None;
        self.manual_scroll_until = Some(now + MANUAL_SCROLL_PAUSE);
    }

    pub(in crate::app) fn resume_follow_now(&mut self) {
        self.manual_scroll_until = None;
        self.scroll_target = None;
        self.follow_resume = true;
    }

    pub(in crate::app) fn is_manual_scroll_paused(&self, now: Instant) -> bool {
        self.manual_scroll_until.is_some_and(|until| now < until)
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
        self.retime(document);
        self.reset_follow();
    }

    /// Like `apply` for a change that only moves timestamps. The viewport stays
    /// put, so the next frame follows the active line from where the list is.
    fn retime(&mut self, document: Document) {
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
    }

    /// Change how the current document is displayed and remember it for this
    /// song. Following restarts, as the change may alter the layout.
    pub(in crate::app) fn change_display(
        &mut self,
        cache: &LyricsCache,
        change: impl FnOnce(&mut Document),
    ) -> anyhow::Result<()> {
        if let Some(document) = self.remembered(cache, change)? {
            self.apply(document);
        }
        Ok(())
    }

    /// Shift the current document in time and remember it for this song,
    /// without moving the lines the listener is reading.
    pub(in crate::app) fn change_timing(
        &mut self,
        cache: &LyricsCache,
        change: impl FnOnce(&mut Document),
    ) -> anyhow::Result<()> {
        if let Some(document) = self.remembered(cache, change)? {
            self.retime(document);
        }
        Ok(())
    }

    /// The changed document, once its preference is saved. A session-only
    /// selection is never saved, and a preference for automatic content does
    /// not pin that content.
    fn remembered(
        &self,
        cache: &LyricsCache,
        change: impl FnOnce(&mut Document),
    ) -> anyhow::Result<Option<Document>> {
        let (Some(media), Some(mut document)) = (&self.media, self.document.clone()) else {
            return Ok(None);
        };
        change(&mut document);
        if !document.temporary {
            let mut preference = document.clone();
            if !document.manual {
                preference.record = None;
            }
            cache.remember(media, &preference)?;
        }
        Ok(Some(document))
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
