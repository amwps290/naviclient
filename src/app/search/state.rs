//! Search results, request lifetime, and session-only display filtering.

use rust_i18n::t;

use crate::app::library::requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use crate::models::SearchResults;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::app) enum SearchFilter {
    #[default]
    All,
    Artists,
    Albums,
    Songs,
}

impl SearchFilter {
    pub(in crate::app) fn label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::All => t!("All"),
            Self::Artists => t!("Artists"),
            Self::Albums => t!("Albums"),
            Self::Songs => t!("Songs"),
        }
    }

    pub(in crate::app) const ALL: [Self; 4] = [Self::All, Self::Artists, Self::Albums, Self::Songs];
}

#[derive(Default)]
pub(in crate::app) struct SearchState {
    load: LibraryLoad,
    results: Option<SearchResults>,
    filter: SearchFilter,
    recent_queries: Vec<String>,
}

const MAX_RECENT_QUERIES: usize = 8;

impl SearchState {
    pub(in crate::app) fn recent_queries(&self) -> &[String] {
        &self.recent_queries
    }

    pub(in crate::app) fn record_query(&mut self, query: &str) {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return;
        }
        self.recent_queries
            .retain(|item| !item.eq_ignore_ascii_case(trimmed));
        self.recent_queries.insert(0, trimmed.to_string());
        self.recent_queries.truncate(MAX_RECENT_QUERIES);
    }

    pub(in crate::app) fn clear_recent_queries(&mut self) {
        self.recent_queries.clear();
    }
    pub(in crate::app) fn results(&self) -> Option<&SearchResults> {
        self.results.as_ref()
    }

    pub(in crate::app) fn loading(&self) -> bool {
        self.load.loading()
    }

    pub(in crate::app) fn filter(&self) -> SearchFilter {
        self.filter
    }

    pub(in crate::app) fn set_filter(&mut self, filter: SearchFilter) -> bool {
        if self.filter == filter {
            return false;
        }
        self.filter = filter;
        true
    }

    pub(in crate::app) fn clear(&mut self) {
        // Empty input invalidates queued results as well as the displayed data.
        // The selected category persists for this server session.
        self.load = LibraryLoad::default();
        self.results = None;
    }

    pub(in crate::app) fn begin(&mut self, requests: &mut LibraryRequests) -> LibraryRequest {
        self.load.begin(requests)
    }

    pub(in crate::app) fn complete(
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
