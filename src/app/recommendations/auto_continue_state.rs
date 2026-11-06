//! Auto-continue request ownership and the queue context captured at its start.

use crate::app::library::requests::{LibraryRequest, LibraryRequests};
use crate::app::queue::logic::auto_continue_context_matches;
use crate::app::songs::list::SongList;
use crate::models::{PlayModes, PlaybackQueue};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AutoContinueRequest {
    request: LibraryRequest,
    server_id: String,
    anchor_song_id: String,
    queue_len: usize,
}

impl AutoContinueRequest {
    pub(in crate::app) fn matches(
        &self,
        server_id: Option<&str>,
        enabled: bool,
        modes: PlayModes,
        index: Option<usize>,
        queue: &PlaybackQueue,
    ) -> bool {
        server_id == Some(self.server_id.as_str())
            && auto_continue_context_matches(
                enabled,
                modes,
                index,
                SongList::Queue(queue.entries()),
                self.queue_len,
                &self.anchor_song_id,
            )
    }
}

#[derive(Default)]
pub(in crate::app) struct AutoContinueState {
    pending: Option<AutoContinueRequest>,
}

impl AutoContinueState {
    pub(in crate::app) fn loading(&self) -> bool {
        self.pending.is_some()
    }

    pub(in crate::app) fn cancel(&mut self) {
        self.pending = None;
    }

    pub(in crate::app) fn begin(
        &mut self,
        server_id: &str,
        queue: &PlaybackQueue,
        requests: &mut LibraryRequests,
    ) -> Option<AutoContinueRequest> {
        if self.loading() {
            return None;
        }
        let anchor_song_id = queue.songs().next_back()?.id.clone();
        let request = AutoContinueRequest {
            request: requests.next(),
            server_id: server_id.to_string(),
            anchor_song_id,
            queue_len: queue.len(),
        };
        self.pending = Some(request.clone());
        Some(request)
    }

    pub(in crate::app) fn complete<T>(
        &mut self,
        request: &AutoContinueRequest,
        result: T,
    ) -> Option<T> {
        if self.pending.as_ref() != Some(request) {
            return None;
        }
        self.pending = None;
        Some(result)
    }
}

#[cfg(test)]
mod tests;
