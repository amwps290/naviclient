//! A cancellable resolution ticket survives page resets and rejects old replies.

use tokio::task::AbortHandle;

use crate::app::library::requests::{LibraryRequest, LibraryRequests};
use crate::models::PlaybackId;
use crate::source::{MediaKey, SourceId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlaybackResolveRequest {
    serial: LibraryRequest,
    media: MediaKey,
    occurrence: Option<PlaybackId>,
}

struct Pending<T> {
    request: PlaybackResolveRequest,
    value: T,
    abort: Option<AbortHandle>,
}

pub(in crate::app) struct ResolutionState<T> {
    pending: Option<Pending<T>>,
    last: Option<PlaybackResolveRequest>,
}

impl<T> Default for ResolutionState<T> {
    fn default() -> Self {
        Self {
            pending: None,
            last: None,
        }
    }
}

impl<T> Drop for ResolutionState<T> {
    fn drop(&mut self) {
        self.cancel();
    }
}

impl<T> ResolutionState<T> {
    pub(in crate::app) fn pending(&self) -> bool {
        self.pending.is_some()
    }

    pub(in crate::app) fn attempted(
        &self,
        media: &MediaKey,
        occurrence: Option<PlaybackId>,
    ) -> bool {
        self.last
            .as_ref()
            .is_some_and(|request| &request.media == media && request.occurrence == occurrence)
    }

    pub(in crate::app) fn begin(
        &mut self,
        media: MediaKey,
        occurrence: Option<PlaybackId>,
        value: T,
        requests: &mut LibraryRequests,
    ) -> PlaybackResolveRequest {
        self.cancel();
        let request = PlaybackResolveRequest {
            serial: requests.next(),
            media,
            occurrence,
        };
        self.last = Some(request.clone());
        self.pending = Some(Pending {
            request: request.clone(),
            value,
            abort: None,
        });
        request
    }

    pub(in crate::app) fn attach(&mut self, request: &PlaybackResolveRequest, abort: AbortHandle) {
        if let Some(pending) = self
            .pending
            .as_mut()
            .filter(|pending| &pending.request == request)
        {
            pending.abort = Some(abort);
        } else {
            abort.abort();
        }
    }

    pub(in crate::app) fn complete(
        &mut self,
        request: &PlaybackResolveRequest,
        occurrence: Option<PlaybackId>,
    ) -> Option<T> {
        if self
            .pending
            .as_ref()
            .is_none_or(|pending| &pending.request != request)
        {
            return None;
        }
        let pending = self.pending.take()?;
        if let Some(abort) = pending.abort {
            abort.abort();
        }
        (request.occurrence == occurrence).then_some(pending.value)
    }

    pub(in crate::app) fn cancel_source(&mut self, source: &SourceId) {
        if self
            .last
            .as_ref()
            .is_some_and(|request| &request.media.source == source)
        {
            self.cancel();
        }
    }

    pub(in crate::app) fn cancel(&mut self) {
        if let Some(pending) = self.pending.take() {
            if let Some(abort) = pending.abort {
                abort.abort();
            }
        }
        self.last = None;
    }
}

#[cfg(test)]
mod tests;
