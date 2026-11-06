//! Search input submission, API requests, and result side effects.

use super::*;

impl AppState {
    pub(super) fn complete_search(
        &mut self,
        request: LibraryRequest,
        result: Result<SearchResults, String>,
    ) -> Option<Result<(), String>> {
        let result = self
            .search
            .complete(request, result)?
            .map_err(str::to_string);
        if result.is_ok() {
            self.clear_song_selection_for_scope(SongScope::Search);
        }
        Some(result)
    }
}

impl NavidromeApp {
    pub(super) fn set_search_filter(&mut self, filter: SearchFilter, cx: &mut Context<Self>) {
        if self.state.search.set_filter(filter) {
            cx.notify();
        }
    }

    pub(super) fn submit_search(&mut self, cx: &Context<Self>) {
        self.clear_song_multi();
        let query = self.search_input.read(cx).value().trim().to_string();
        if query.is_empty() {
            self.state.search.clear();
            self.state.clear_song_selection_for_scope(SongScope::Search);
            return;
        }
        let Some(api) = self.api.clone() else { return };
        let request = self.state.search.begin(&mut self.library_requests);
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.search(&query, 120, 80, 60).await.map_err(error_message);
            let _ = tx.send(Msg::Search { request, result });
        });
    }

    pub(super) fn finish_search(
        &mut self,
        request: LibraryRequest,
        result: Result<SearchResults, String>,
    ) {
        if let Some(Err(error)) = self.state.complete_search(request, result) {
            self.notify_error(error);
        }
    }
}
