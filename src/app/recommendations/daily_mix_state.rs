//! Daily mix snapshots, request context, and deferred playback intent.

use std::collections::HashSet;

use crate::app::library::requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use crate::recommendation::DailyMixSnapshot;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DailyMixRequest {
    request: LibraryRequest,
    server_id: String,
    day: String,
    variant: u32,
}

impl DailyMixRequest {
    pub(in crate::app) fn server_id(&self) -> &str {
        &self.server_id
    }
}

struct PendingMix {
    request: DailyMixRequest,
    play_when_ready: bool,
}

#[derive(Default)]
pub(in crate::app) struct DailyMixState {
    snapshot: Option<DailyMixSnapshot>,
    previous_ids: HashSet<String>,
    load: LibraryLoad,
    pending: Option<PendingMix>,
}

impl DailyMixState {
    pub(in crate::app) fn snapshot(&self) -> Option<&DailyMixSnapshot> {
        self.snapshot.as_ref()
    }

    pub(in crate::app) fn loading(&self) -> bool {
        self.load.loading()
    }

    pub(in crate::app) fn error(&self) -> Option<&str> {
        self.load.error()
    }

    pub(in crate::app) fn recent_song_ids(&self) -> impl Iterator<Item = &String> {
        self.previous_ids.iter().chain(
            self.snapshot
                .iter()
                .flat_map(|snapshot| snapshot.tracks.iter().map(|track| &track.song.id)),
        )
    }

    pub(in crate::app) fn restore(
        &mut self,
        cached: Option<DailyMixSnapshot>,
        server_id: &str,
        day: &str,
    ) {
        *self = Self::default();
        if let Some(snapshot) = cached.filter(|snapshot| snapshot.server_id == server_id) {
            self.previous_ids = snapshot
                .tracks
                .iter()
                .map(|track| track.song.id.clone())
                .collect();
            if snapshot.is_current(server_id, day) {
                self.snapshot = Some(snapshot);
            }
        }
    }

    pub(in crate::app) fn expire(&mut self, server_id: &str, day: &str) {
        if self
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| !snapshot.is_current(server_id, day))
        {
            let snapshot = self.snapshot.take().expect("stale snapshot exists");
            self.previous_ids = snapshot
                .tracks
                .into_iter()
                .map(|track| track.song.id)
                .collect();
        }
    }

    pub(in crate::app) fn refresh_variant(&self, server_id: &str, day: &str) -> u32 {
        self.snapshot
            .as_ref()
            .filter(|snapshot| snapshot.is_current(server_id, day))
            .map_or(0, |snapshot| snapshot.variant.wrapping_add(1))
    }

    pub(in crate::app) fn retry_variant(&self) -> u32 {
        self.snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.variant)
    }

    pub(in crate::app) fn cancel_playback(&mut self) {
        if let Some(pending) = &mut self.pending {
            pending.play_when_ready = false;
        }
    }

    pub(in crate::app) fn begin(
        &mut self,
        server_id: &str,
        day: &str,
        variant: u32,
        play_when_ready: bool,
        requests: &mut LibraryRequests,
    ) -> Option<DailyMixRequest> {
        if let Some(pending) = &mut self.pending {
            if pending.request.server_id == server_id && pending.request.day == day {
                pending.play_when_ready |= play_when_ready;
                return None;
            }
        }
        let request = DailyMixRequest {
            request: self.load.begin(requests),
            server_id: server_id.to_string(),
            day: day.to_string(),
            variant,
        };
        self.pending = Some(PendingMix {
            request: request.clone(),
            play_when_ready,
        });
        Some(request)
    }

    /// Accepted success returns whether to play. Errors retain the previous
    /// snapshot, and stale/duplicate responses cannot consume a newer intent.
    pub(in crate::app) fn complete(
        &mut self,
        request: &DailyMixRequest,
        result: Result<DailyMixSnapshot, String>,
    ) -> Option<Result<bool, &str>> {
        if self.pending.as_ref().map(|pending| &pending.request) != Some(request) {
            return None;
        }
        let result = self.load.complete(request.request, result)?;
        let pending = self
            .pending
            .take()
            .expect("matching daily mix request exists");
        Some(match result {
            Ok(snapshot) => {
                if let Some(previous) = self.snapshot.replace(snapshot) {
                    self.previous_ids = previous
                        .tracks
                        .into_iter()
                        .map(|track| track.song.id)
                        .collect();
                }
                Ok(pending.play_when_ready)
            }
            Err(error) => Err(error),
        })
    }
}

#[cfg(test)]
mod tests;
