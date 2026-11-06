//! Radio request identity, typed targets, and seed queue ownership.

use super::library_requests::{LibraryRequest, LibraryRequests};
use crate::models::{PlaybackQueue, QueueOrigin, Song};
use crate::recommendation::{RadioSeed, RecommendedTrack};

#[derive(Clone, Debug, PartialEq, Eq)]
enum RadioTarget {
    Song(String),
    Artist(String),
}

impl RadioTarget {
    fn from_seed(seed: &RadioSeed) -> Self {
        match seed {
            RadioSeed::Song(song) => Self::Song(song.id.clone()),
            RadioSeed::Artist(artist) => Self::Artist(artist.id.clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RadioRequest {
    request: LibraryRequest,
    server_id: String,
    target: RadioTarget,
    initial_song_id: String,
}

impl RadioRequest {
    pub(super) fn is_artist(&self) -> bool {
        matches!(self.target, RadioTarget::Artist(_))
    }

    pub(super) fn origin(&self) -> QueueOrigin {
        match self.target {
            RadioTarget::Song(_) => QueueOrigin::SongRadio,
            RadioTarget::Artist(_) => QueueOrigin::ArtistRadio,
        }
    }

    fn seed_queue_matches(
        &self,
        index: Option<usize>,
        queue: &PlaybackQueue,
        now_playing: Option<&Song>,
    ) -> bool {
        queue.len() == 1
            && index == Some(0)
            && queue
                .songs()
                .next()
                .is_some_and(|song| song.id == self.initial_song_id)
            && now_playing.is_some_and(|song| song.id == self.initial_song_id)
    }
}

#[derive(Default)]
pub(super) struct RadioState {
    pending: Option<RadioRequest>,
}

impl RadioState {
    pub(super) fn song_loading(&self, song_id: &str) -> bool {
        self.pending.as_ref().is_some_and(
            |request| matches!(&request.target, RadioTarget::Song(id) if id == song_id),
        )
    }

    pub(super) fn artist_loading(&self, artist_id: &str) -> bool {
        self.pending.as_ref().is_some_and(
            |request| matches!(&request.target, RadioTarget::Artist(id) if id == artist_id),
        )
    }

    pub(super) fn cancel(&mut self) {
        self.pending = None;
    }

    pub(super) fn begin(
        &mut self,
        server_id: &str,
        seed: &RadioSeed,
        initial_song: &Song,
        requests: &mut LibraryRequests,
    ) -> RadioRequest {
        let request = RadioRequest {
            request: requests.next(),
            server_id: server_id.to_string(),
            target: RadioTarget::from_seed(seed),
            initial_song_id: initial_song.id.clone(),
        };
        self.pending = Some(request.clone());
        request
    }

    pub(super) fn complete(
        &mut self,
        request: &RadioRequest,
        server_id: Option<&str>,
        index: Option<usize>,
        queue: &PlaybackQueue,
        now_playing: Option<&Song>,
        result: Result<Vec<RecommendedTrack>, String>,
    ) -> Option<Result<Vec<RecommendedTrack>, String>> {
        if self.pending.as_ref() != Some(request) || server_id != Some(request.server_id.as_str()) {
            return None;
        }
        self.pending = None;
        request
            .seed_queue_matches(index, queue, now_playing)
            .then_some(result)
    }
}

#[cfg(test)]
mod tests;
