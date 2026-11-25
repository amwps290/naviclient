//! Search revisions and borrowed occurrence mappings owned by a playlist snapshot.

use super::search_index::{Cancellation, PlaylistSearchIndex, QueryTerms};
use crate::app::LibraryRequest;
use std::sync::Arc;

/// Snapshot IDs survive source/page resets; revisions invalidate stale row events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PlaylistSearchRequest {
    pub snapshot: LibraryRequest,
    pub revision: u64,
}

pub(in crate::app) struct SearchWork {
    pub abort: tokio::task::AbortHandle,
    pub cancel: Cancellation,
}

impl Drop for SearchWork {
    fn drop(&mut self) {
        self.cancel.cancel();
        self.abort.abort();
    }
}

#[derive(Default)]
pub(in crate::app) struct PlaylistSearch {
    pub expanded: bool,
    pub query: String,
    terms: QueryTerms,
    revision: u64,
    snapshot: Option<LibraryRequest>,
    index: Option<Arc<PlaylistSearchIndex>>,
    indices: Option<Arc<[usize]>>,
    pending: bool,
    error: Option<String>,
    work: Option<SearchWork>,
    pub before_search_row: Option<usize>,
    pub located: Option<usize>,
}

impl PlaylistSearch {
    fn invalidate(&mut self) {
        self.work = None;
        self.revision = self
            .revision
            .checked_add(1)
            .expect("playlist query revision exhausted");
    }

    pub fn identity(&self, snapshot: LibraryRequest) -> PlaylistSearchRequest {
        PlaylistSearchRequest {
            snapshot,
            revision: self.revision,
        }
    }

    pub fn set_query(&mut self, query: String) -> bool {
        let terms = QueryTerms::new(&query);
        if !terms.is_empty() {
            self.expanded = true;
        }
        self.query = query;
        if terms == self.terms {
            return false;
        }
        self.invalidate();
        self.terms = terms;
        self.error = None;
        self.pending = !self.terms.is_empty();
        if !self.pending {
            self.indices = None;
        }
        true
    }

    pub fn replace(&mut self, snapshot: LibraryRequest, index: Arc<PlaylistSearchIndex>) {
        self.reset_snapshot();
        self.snapshot = Some(snapshot);
        self.index = Some(index);
    }

    pub fn reset_snapshot(&mut self) {
        self.invalidate();
        self.snapshot = None;
        self.index = None;
        self.indices = None;
        self.error = None;
        self.pending = self.filtered();
        self.before_search_row = None;
        self.located = None;
    }

    pub fn suspend(&mut self) {
        self.invalidate();
    }

    pub fn retry(&mut self) {
        self.invalidate();
        self.error = None;
        self.pending = self.filtered();
    }

    pub fn start(&self) -> Option<(PlaylistSearchRequest, Arc<PlaylistSearchIndex>, QueryTerms)> {
        if !self.pending || self.work.is_some() || self.error.is_some() {
            return None;
        }
        Some((
            self.identity(self.snapshot?),
            self.index.as_ref()?.clone(),
            self.terms.clone(),
        ))
    }

    pub fn attach(&mut self, work: SearchWork) {
        self.work = Some(work);
    }

    pub fn complete(
        &mut self,
        request: PlaylistSearchRequest,
        result: Result<Vec<usize>, String>,
    ) -> bool {
        if self.snapshot.map(|snapshot| self.identity(snapshot)) != Some(request) || !self.pending {
            return false;
        }
        self.work = None;
        self.pending = false;
        match result {
            Ok(indices)
                if indices.windows(2).all(|w| w[0] < w[1])
                    && indices.last().is_none_or(|i| {
                        *i < self.index.as_ref().map_or(0, |index| index.len())
                    }) =>
            {
                self.indices = Some(indices.into());
                self.error = None;
            }
            Ok(_) => self.error = Some("Invalid playlist search result".into()),
            Err(error) => self.error = Some(error),
        }
        true
    }

    pub fn indexed(&self, snapshot: LibraryRequest) -> bool {
        self.snapshot == Some(snapshot) && self.index.is_some()
    }

    pub fn filtered(&self) -> bool {
        !self.terms.is_empty()
    }
    pub fn pending(&self) -> bool {
        self.pending
    }
    pub fn ready(&self) -> bool {
        !self.pending && self.error.is_none()
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
    pub fn indices(&self) -> Option<&[usize]> {
        self.indices.as_deref()
    }
    pub fn original_index(&self, visible: usize, len: usize) -> Option<usize> {
        let index = match self.indices() {
            Some(indices) => *indices.get(visible)?,
            None => visible,
        };
        (index < len).then_some(index)
    }
}

#[cfg(test)]
mod tests;
