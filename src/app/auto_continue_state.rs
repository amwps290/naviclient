//! Auto-continue request ownership and the queue context captured at its start.

use super::library_requests::{LibraryRequest, LibraryRequests};
use super::queue_logic::auto_continue_context_matches;
use super::song_list::SongList;
use crate::models::{PlayModes, PlaybackQueue};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AutoContinueRequest {
    request: LibraryRequest,
    server_id: String,
    anchor_song_id: String,
    queue_len: usize,
}

impl AutoContinueRequest {
    pub(super) fn matches(
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
pub(super) struct AutoContinueState {
    pending: Option<AutoContinueRequest>,
}

impl AutoContinueState {
    pub(super) fn loading(&self) -> bool {
        self.pending.is_some()
    }

    pub(super) fn cancel(&mut self) {
        self.pending = None;
    }

    pub(super) fn begin(
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

    pub(super) fn complete<T>(&mut self, request: &AutoContinueRequest, result: T) -> Option<T> {
        if self.pending.as_ref() != Some(request) {
            return None;
        }
        self.pending = None;
        Some(result)
    }
}

#[cfg(test)]
mod tests;
