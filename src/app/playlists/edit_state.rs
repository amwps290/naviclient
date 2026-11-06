//! A playlist write keeps its initiating workflow until its result is consumed.

use crate::app::library::requests::{LibraryRequest, LibraryRequests};
use crate::playlist::PlaylistEdited;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum PlaylistEditOrigin {
    Menu,
    Dialog,
    Import,
}

struct PendingEdit {
    request: LibraryRequest,
    origin: PlaylistEditOrigin,
}

pub(in crate::app) struct PlaylistEditCompletion {
    pub(in crate::app) origin: PlaylistEditOrigin,
    pub(in crate::app) result: Result<PlaylistEdited, String>,
}

#[derive(Default)]
pub(in crate::app) struct PlaylistEdits {
    pending: Option<PendingEdit>,
}

impl PlaylistEdits {
    pub(in crate::app) fn editing(&self) -> bool {
        self.pending.is_some()
    }

    pub(in crate::app) fn begin(
        &mut self,
        requests: &mut LibraryRequests,
        origin: PlaylistEditOrigin,
    ) -> Option<LibraryRequest> {
        if self.editing() {
            return None;
        }
        let request = requests.next();
        self.pending = Some(PendingEdit { request, origin });
        Some(request)
    }

    pub(in crate::app) fn complete(
        &mut self,
        request: LibraryRequest,
        result: Result<PlaylistEdited, String>,
    ) -> Option<PlaylistEditCompletion> {
        if !self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.request == request)
        {
            return None;
        }
        let pending = self.pending.take().expect("accepted playlist edit exists");
        Some(PlaylistEditCompletion {
            origin: pending.origin,
            result,
        })
    }
}
