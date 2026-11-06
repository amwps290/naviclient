//! Import phases bind request identity, source tracks, progress, and review data.

use rust_i18n::t;
use tokio::task::AbortHandle;

use crate::app::library::requests::{LibraryRequest, LibraryRequests};
use crate::import::{ImportedPlaylist, ImportedTrack, MatchedSongs, TrackMatch};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum ImportStage {
    Url,
    Matching,
    Review,
}

pub(in crate::app) struct ImportReviewTrack {
    pub(in crate::app) source: ImportedTrack,
    pub(in crate::app) outcome: TrackMatch,
}

pub(in crate::app) struct ImportReview {
    pub(in crate::app) name: String,
    pub(in crate::app) unavailable: usize,
    pub(in crate::app) rows: Vec<ImportReviewTrack>,
}

#[derive(Default)]
pub(in crate::app) struct ImportCounts {
    pub(in crate::app) matched: usize,
    pub(in crate::app) unmatched: usize,
    pub(in crate::app) failed: usize,
}

impl ImportReview {
    pub(in crate::app) fn counts(&self) -> ImportCounts {
        let mut counts = ImportCounts::default();
        for row in &self.rows {
            match row.outcome {
                TrackMatch::Matched(_) => counts.matched += 1,
                TrackMatch::Unmatched => counts.unmatched += 1,
                TrackMatch::Failed => counts.failed += 1,
            }
        }
        counts
    }

    pub(in crate::app) fn song_ids(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter_map(|row| row.outcome.song().map(|song| song.id.clone()))
            .collect()
    }
}

struct PendingImport {
    request: LibraryRequest,
    task: Option<AbortHandle>,
}

#[derive(Default)]
enum ImportPhase {
    #[default]
    Closed,
    Url,
    Resolving(PendingImport),
    Matching {
        pending: PendingImport,
        playlist: ImportedPlaylist,
        done: usize,
    },
    Review(ImportReview),
}

#[derive(Default)]
pub(in crate::app) struct ImportFlow {
    phase: ImportPhase,
    error: Option<String>,
}

impl ImportFlow {
    pub(in crate::app) fn stage(&self) -> Option<ImportStage> {
        match self.phase {
            ImportPhase::Closed => None,
            ImportPhase::Url => Some(ImportStage::Url),
            ImportPhase::Resolving(_) | ImportPhase::Matching { .. } => Some(ImportStage::Matching),
            ImportPhase::Review(_) => Some(ImportStage::Review),
        }
    }

    pub(in crate::app) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(in crate::app) fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    pub(in crate::app) fn clear_error(&mut self) {
        self.error = None;
    }

    pub(in crate::app) fn review(&self) -> Option<&ImportReview> {
        match &self.phase {
            ImportPhase::Review(review) => Some(review),
            _ => None,
        }
    }

    pub(in crate::app) fn matching_playlist(&self) -> Option<&ImportedPlaylist> {
        match &self.phase {
            ImportPhase::Matching { playlist, .. } => Some(playlist),
            _ => None,
        }
    }

    pub(in crate::app) fn progress(&self) -> (usize, usize) {
        match &self.phase {
            ImportPhase::Matching { playlist, done, .. } => (*done, playlist.tracks.len()),
            _ => (0, 0),
        }
    }

    pub(in crate::app) fn open(&mut self) {
        self.reset();
        self.phase = ImportPhase::Url;
    }

    pub(in crate::app) fn reset(&mut self) {
        self.abort_task();
        self.phase = ImportPhase::Closed;
        self.error = None;
    }

    pub(in crate::app) fn begin_resolve(
        &mut self,
        requests: &mut LibraryRequests,
    ) -> Option<LibraryRequest> {
        if !matches!(self.phase, ImportPhase::Url) {
            return None;
        }
        let request = requests.next();
        self.phase = ImportPhase::Resolving(PendingImport {
            request,
            task: None,
        });
        self.error = None;
        Some(request)
    }

    pub(in crate::app) fn set_task(&mut self, request: LibraryRequest, task: AbortHandle) {
        if let Some(pending) = self
            .pending_mut()
            .filter(|pending| pending.request == request)
        {
            if let Some(previous) = pending.task.replace(task) {
                previous.abort();
            }
        } else {
            task.abort();
        }
    }

    pub(in crate::app) fn complete_resolve(
        &mut self,
        request: LibraryRequest,
        result: Result<ImportedPlaylist, String>,
        requests: &mut LibraryRequests,
    ) -> Option<Result<LibraryRequest, &str>> {
        if !matches!(&self.phase, ImportPhase::Resolving(pending) if pending.request == request) {
            return None;
        }
        Some(match result {
            Ok(playlist) => {
                let request = requests.next();
                self.phase = ImportPhase::Matching {
                    pending: PendingImport {
                        request,
                        task: None,
                    },
                    playlist,
                    done: 0,
                };
                Ok(request)
            }
            Err(error) => {
                self.phase = ImportPhase::Url;
                self.error = Some(error);
                Err(self
                    .error
                    .as_deref()
                    .expect("failed import stores its error"))
            }
        })
    }

    pub(in crate::app) fn update_progress(&mut self, request: LibraryRequest, done: usize) {
        if let ImportPhase::Matching {
            pending,
            playlist,
            done: current,
        } = &mut self.phase
        {
            if pending.request == request {
                // Workers can enqueue callbacks out of order after incrementing
                // their shared counter. Never move displayed progress backward.
                *current = (*current).max(done.min(playlist.tracks.len()));
            }
        }
    }

    pub(in crate::app) fn complete_match(
        &mut self,
        request: LibraryRequest,
        result: Result<MatchedSongs, String>,
    ) -> bool {
        if !matches!(&self.phase, ImportPhase::Matching { pending, .. } if pending.request == request)
        {
            return false;
        }
        let ImportPhase::Matching { playlist, .. } =
            std::mem::replace(&mut self.phase, ImportPhase::Url)
        else {
            unreachable!("matching phase was checked")
        };
        match result {
            Ok(result) if result.entries.len() == playlist.tracks.len() => {
                let review = ImportReview {
                    name: playlist.name,
                    unavailable: playlist.unavailable_tracks,
                    rows: playlist
                        .tracks
                        .into_iter()
                        .zip(result.entries)
                        .map(|(source, outcome)| ImportReviewTrack { source, outcome })
                        .collect(),
                };
                let counts = review.counts();
                if counts.matched == 0 {
                    self.error = Some(if counts.failed > 0 {
                        t!("No tracks are ready to import. Cancel and try again.").to_string()
                    } else {
                        t!("No tracks matched your library.").to_string()
                    });
                }
                self.phase = ImportPhase::Review(review);
            }
            Ok(_) => {
                self.error =
                    Some(t!("No tracks are ready to import. Cancel and try again.").to_string())
            }
            Err(error) => self.error = Some(error),
        }
        true
    }

    fn pending_mut(&mut self) -> Option<&mut PendingImport> {
        match &mut self.phase {
            ImportPhase::Resolving(pending) | ImportPhase::Matching { pending, .. } => {
                Some(pending)
            }
            _ => None,
        }
    }

    fn abort_task(&mut self) {
        if let Some(task) = self.pending_mut().and_then(|pending| pending.task.take()) {
            task.abort();
        }
    }
}

impl Drop for ImportFlow {
    fn drop(&mut self) {
        self.abort_task();
    }
}

#[cfg(test)]
mod tests;
