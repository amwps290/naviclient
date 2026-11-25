//! Source-bound local Bilibili browsing state, independent of playback ownership.

use crate::app::library::requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use crate::models::{ListeningHistory, Playlist, Song};
use crate::source::SourceId;
use unicode_normalization::UnicodeNormalization;

#[derive(Default)]
pub(in crate::app) struct BilibiliWorkspace {
    source: Option<SourceId>,
    pub history: ListeningHistory,
    history_load: LibraryLoad,
    history_task: Option<tokio::task::AbortHandle>,
    pub folder_query: String,
    pub folder_limit: usize,
    pub visited: Vec<String>,
    cover_snapshot: Option<LibraryRequest>,
    cover_attempted: std::collections::HashSet<String>,
    cover_jobs: std::collections::HashMap<String, (LibraryRequest, tokio::task::AbortHandle)>,
    pub cover_failures: usize,
    cover_stopped: bool,
    cover_stop_signal: std::sync::Arc<std::sync::atomic::AtomicBool>,
    query_changed_at: Option<std::time::Instant>,
}

impl Drop for BilibiliWorkspace {
    fn drop(&mut self) {
        if let Some(task) = self.history_task.take() {
            task.abort();
        }
        for (_, task) in self.cover_jobs.values() {
            task.abort();
        }
    }
}

impl BilibiliWorkspace {
    pub fn begin_history(
        &mut self,
        source: SourceId,
        requests: &mut LibraryRequests,
    ) -> LibraryRequest {
        if self.source.as_ref() != Some(&source) {
            *self = Self::default();
            self.source = Some(source);
            self.folder_limit = 50;
        }
        if let Some(task) = self.history_task.take() {
            task.abort();
        }
        self.history_load.begin(requests)
    }
    pub fn attach_history(&mut self, task: tokio::task::AbortHandle) {
        self.history_task = Some(task);
    }
    pub fn complete_history(
        &mut self,
        source: &SourceId,
        request: LibraryRequest,
        result: Result<ListeningHistory, String>,
    ) -> bool {
        if self.source.as_ref() != Some(source) {
            return false;
        }
        let Some(result) = self.history_load.complete(request, result) else {
            return false;
        };
        self.history_task = None;
        if let Ok(mut history) = result {
            history
                .events
                .retain(|event| event.song.source.as_ref() == Some(source));
            self.history = history;
        }
        true
    }
    pub fn history_loading(&self) -> bool {
        self.history_load.loading()
    }
    pub fn history_error(&self) -> Option<&str> {
        self.history_load.error()
    }
    pub fn sync_cover_snapshot(&mut self, snapshot: Option<LibraryRequest>) {
        if self.cover_snapshot == snapshot {
            return;
        }
        for (_, task) in self.cover_jobs.values() {
            task.abort();
        }
        self.cover_jobs.clear();
        self.cover_attempted.clear();
        self.cover_failures = 0;
        self.cover_stopped = false;
        self.cover_stop_signal = Default::default();
        self.cover_snapshot = snapshot;
    }
    pub fn prune_cover_jobs(&mut self, wanted: &std::collections::HashSet<String>) {
        let obsolete = self
            .cover_jobs
            .keys()
            .filter(|id| !wanted.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        for id in obsolete {
            if let Some((_, task)) = self.cover_jobs.remove(&id) {
                task.abort();
            }
            self.cover_attempted.remove(&id);
        }
    }
    pub fn cover_query_settled(&self) -> bool {
        self.query_changed_at
            .is_none_or(|at| at.elapsed() >= std::time::Duration::from_millis(200))
    }
    pub fn cover_stop_signal(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        self.cover_stop_signal.clone()
    }
    pub fn needs_cover(&self, id: &str) -> bool {
        !self.cover_stopped
            && !self
                .cover_stop_signal
                .load(std::sync::atomic::Ordering::Relaxed)
            && !self.cover_attempted.contains(id)
    }
    pub fn attach_cover(
        &mut self,
        id: String,
        request: LibraryRequest,
        task: tokio::task::AbortHandle,
    ) {
        self.cover_attempted.insert(id.clone());
        self.cover_jobs.insert(id, (request, task));
    }
    pub fn finish_cover(&mut self, id: &str, request: LibraryRequest) -> bool {
        if self
            .cover_jobs
            .get(id)
            .is_none_or(|(current, _)| *current != request)
        {
            return false;
        }
        self.cover_jobs.remove(id);
        true
    }
    pub fn cover_failed(&mut self, stop: bool) {
        self.cover_failures += 1;
        if stop {
            self.cover_stopped = true;
            self.cover_stop_signal
                .store(true, std::sync::atomic::Ordering::Relaxed);
            for (_, task) in self.cover_jobs.values() {
                task.abort();
            }
            self.cover_jobs.clear();
        }
    }

    pub fn set_folder_query(&mut self, query: String) {
        if self.folder_query != query {
            self.query_changed_at = Some(std::time::Instant::now());
        }
        self.folder_query = query;
        self.folder_limit = 50;
    }
    pub fn show_more_folders(&mut self) {
        self.folder_limit = self.folder_limit.max(50).saturating_add(100);
    }
    pub fn visit(&mut self, id: &str) {
        self.visited.retain(|previous| previous != id);
        self.visited.insert(0, id.into());
        self.visited.truncate(6);
    }
}

pub(in crate::app) fn recent_tracks<'a>(
    history: &'a ListeningHistory,
    source: &SourceId,
) -> Vec<&'a Song> {
    let mut ids = std::collections::HashSet::new();
    history
        .events
        .iter()
        .rev()
        .filter(|event| {
            event.song.source.as_ref() == Some(source) && ids.insert(event.song.id.as_str())
        })
        .take(6)
        .map(|event| &event.song)
        .collect()
}

pub(in crate::app) fn matching_folders<'a>(
    folders: &'a [Playlist],
    query: &str,
) -> Vec<&'a Playlist> {
    let normalize = |text: &str| text.nfkc().flat_map(char::to_lowercase).collect::<String>();
    let normalized = normalize(query);
    let terms = normalized.split_whitespace().collect::<Vec<_>>();
    folders
        .iter()
        .filter(|folder| {
            let name = normalize(&folder.name);
            terms.iter().all(|term| name.contains(term))
        })
        .collect()
}

#[cfg(test)]
mod tests;
