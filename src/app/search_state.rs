//! Search results, request lifetime, and session-only display filtering.

use rust_i18n::t;

use super::library_requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use crate::models::SearchResults;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SearchFilter {
    #[default]
    All,
    Artists,
    Albums,
    Songs,
}

impl SearchFilter {
    pub(super) fn label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::All => t!("All"),
            Self::Artists => t!("Artists"),
            Self::Albums => t!("Albums"),
            Self::Songs => t!("Songs"),
        }
    }

    pub(super) const ALL: [Self; 4] = [Self::All, Self::Artists, Self::Albums, Self::Songs];
}

#[derive(Default)]
pub(super) struct SearchState {
    load: LibraryLoad,
    results: Option<SearchResults>,
    filter: SearchFilter,
}

impl SearchState {
    pub(super) fn results(&self) -> Option<&SearchResults> {
        self.results.as_ref()
    }

    pub(super) fn loading(&self) -> bool {
        self.load.loading()
    }

    pub(super) fn filter(&self) -> SearchFilter {
        self.filter
    }

    pub(super) fn set_filter(&mut self, filter: SearchFilter) -> bool {
        if self.filter == filter {
            return false;
        }
        self.filter = filter;
        true
    }

    pub(super) fn clear(&mut self) {
        // Empty input invalidates queued results as well as the displayed data.
        // The selected category persists for this server session.
        self.load = LibraryLoad::default();
        self.results = None;
    }

    pub(super) fn begin(&mut self, requests: &mut LibraryRequests) -> LibraryRequest {
        self.load.begin(requests)
    }

    pub(super) fn complete(
        &mut self,
        request: LibraryRequest,
        result: Result<SearchResults, String>,
    ) -> Option<Result<(), &str>> {
        Some(match self.load.complete(request, result)? {
            Ok(results) => {
                self.results = Some(results);
                Ok(())
            }
            // A failed refresh leaves the last successful results available.
            Err(error) => Err(error),
        })
    }
}

#[cfg(test)]
mod tests;
