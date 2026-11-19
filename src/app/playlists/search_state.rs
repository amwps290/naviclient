//! Search revisions and borrowed occurrence mappings owned by a playlist snapshot.

use super::search_index::{Cancellation, PlaylistSearchIndex, QueryTerms};
use crate::app::LibraryRequest;
use crate::playlist::PlaylistDetails;
use std::sync::Arc;

pub(crate) struct PlaylistLoaded {
    pub(in crate::app) details: PlaylistDetails,
    pub(in crate::app) index: Arc<PlaylistSearchIndex>,
}

impl PlaylistLoaded {
    pub(in crate::app) fn build(details: PlaylistDetails, cancel: &Cancellation) -> Option<Self> {
        let index = Arc::new(PlaylistSearchIndex::build(&details.songs, cancel)?);
        Some(Self { details, index })
    }
}

/// Snapshot IDs come from the app's counter, which survives source/page resets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PlaylistSearchRequest {
    pub(in crate::app) snapshot: LibraryRequest,
    pub(in crate::app) revision: u64,
}

pub(in crate::app) struct SearchWork {
    pub abort: tokio::task::AbortHandle,
    pub cancel: Cancellation,
}
impl Drop for SearchWork {
    fn drop(&mut self) { self.cancel.cancel(); self.abort.abort(); }
}

#[derive(Default)]
pub(in crate::app) struct PlaylistSearch {
    pub query: String,
    terms: QueryTerms,
    view: Option<PlaylistSearchRequest>,
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
        if let Some(view) = self.view.as_mut() {
            view.revision = view.revision.checked_add(1).expect("playlist query revision exhausted");
        }
    }

    pub fn set_query(&mut self, query: String) -> bool {
        let terms = QueryTerms::new(&query);
        self.query = query;
        if terms == self.terms { return false; }
        self.invalidate();
        self.terms = terms;
        self.error = None;
        self.pending = !self.terms.is_empty();
        if !self.pending { self.indices = None; }
        true
    }

    pub fn replace(&mut self, snapshot: LibraryRequest, index: Arc<PlaylistSearchIndex>) {
        self.work = None;
        self.view = Some(PlaylistSearchRequest { snapshot, revision: 0 });
        self.index = Some(index);
        self.indices = None;
        self.error = None;
        self.pending = !self.terms.is_empty();
        self.before_search_row = None;
        self.located = None;
    }

    pub fn suspend(&mut self) {
        self.invalidate();
        self.pending = !self.terms.is_empty() && (self.pending || self.error.is_some());
    }

    pub fn retry(&mut self) {
        self.invalidate();
        self.error = None;
        self.pending = !self.terms.is_empty();
    }

    pub fn start(&self) -> Option<(PlaylistSearchRequest, Arc<PlaylistSearchIndex>, QueryTerms)> {
        if !self.pending || self.work.is_some() { return None; }
        Some((self.view?, self.index.as_ref()?.clone(), self.terms.clone()))
    }

    pub fn attach(&mut self, work: SearchWork) { self.work = Some(work); }

    pub fn complete(&mut self, request: PlaylistSearchRequest, result: Result<Vec<usize>, String>) -> bool {
        if self.view != Some(request) || !self.pending { return false; }
        self.work = None;
        self.pending = false;
        match result {
            Ok(indices) if indices.windows(2).all(|w| w[0] < w[1])
                && indices.last().is_none_or(|i| *i < self.index.as_ref().map_or(0, |index| index.len())) => {
                self.indices = Some(indices.into());
                self.error = None;
            }
            Ok(_) => self.error = Some("Invalid playlist search result".into()),
            Err(error) => self.error = Some(error),
        }
        true
    }

    pub fn filtered(&self) -> bool { !self.terms.is_empty() }
    pub fn pending(&self) -> bool { self.pending }
    pub fn ready(&self) -> bool { self.view.is_some() && !self.pending && self.error.is_none() }
    pub fn error(&self) -> Option<&str> { self.error.as_deref() }
    pub fn view(&self) -> Option<PlaylistSearchRequest> { self.view }
    pub fn indices(&self) -> Option<&[usize]> { self.indices.as_deref() }
    pub fn original_index(&self, visible: usize, len: usize) -> Option<usize> {
        let index = match self.indices() { Some(indices) => *indices.get(visible)?, None => visible };
        (index < len).then_some(index)
    }
    pub fn visible_index(&self, original: usize, len: usize) -> Option<usize> {
        if original >= len { return None; }
        match self.indices() { Some(indices) => indices.binary_search(&original).ok(), None => Some(original) }
    }
}

#[cfg(test)]
mod tests;
