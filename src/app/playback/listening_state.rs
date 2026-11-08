//! Server-scoped listening events, actual progress, and the History drawer projection.

use std::time::{Duration, Instant};

use crate::audio::PlaybackState;
use crate::models::{ListeningHistory, PlaybackId, Song};
use crate::source::{MediaKey, SourceId};

const HISTORY_DISPLAY_LIMIT: usize = 200;
const PROGRESS_SAVE_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Default)]
pub(in crate::app) struct ListeningState {
    server_id: Option<String>,
    history: ListeningHistory,
    recent: Vec<Song>,
    active: Option<ActiveListening>,
}

struct ActiveListening {
    source: Option<SourceId>,
    occurrence: PlaybackId,
    song_id: String,
    last_position: Duration,
    last_seek_serial: u64,
    accumulated: Duration,
    last_saved: Instant,
}

impl ActiveListening {
    fn update_history(&self, history: &mut ListeningHistory, completed: bool) {
        let listened_seconds = u32::try_from(self.accumulated.as_secs()).unwrap_or(u32::MAX);
        if let Some(source) = self.source.as_ref() {
            history.update_latest_media(
                &MediaKey::new(source.clone(), &self.song_id),
                listened_seconds,
                completed,
            );
        } else {
            history.update_latest(&self.song_id, listened_seconds, completed);
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(in crate::app) enum ListeningUpdate {
    Unchanged,
    Started,
    Updated,
}

impl ListeningState {
    pub(in crate::app) fn server_id(&self) -> Option<&str> {
        self.server_id.as_deref()
    }

    pub(in crate::app) fn history(&self) -> &ListeningHistory {
        &self.history
    }

    pub(in crate::app) fn recent_songs(&self) -> &[Song] {
        &self.recent
    }

    /// Restoring a server's history never resumes an in-memory listening event.
    pub(in crate::app) fn restore(&mut self, server_id: Option<&str>, history: ListeningHistory) {
        self.server_id = server_id.map(str::to_string);
        self.recent = history.recent_songs(HISTORY_DISPLAY_LIMIT);
        self.history = history;
        self.active = None;
    }

    /// Observe actual output progress, after pending transport intent is excluded.
    pub(in crate::app) fn observe(
        &mut self,
        song: Option<&Song>,
        occurrence: Option<PlaybackId>,
        playback: &PlaybackState,
        now: Instant,
        started_at: impl FnOnce() -> u64,
    ) -> ListeningUpdate {
        let Some((song, occurrence)) = song
            .filter(|song| !song.id.trim().is_empty())
            .zip(occurrence)
        else {
            return if self.finish(false) {
                ListeningUpdate::Updated
            } else {
                ListeningUpdate::Unchanged
            };
        };
        if self.active.as_ref().is_none_or(|active| {
            active.source != song.source
                || active.song_id != song.id
                || active.occurrence != occurrence
        }) {
            let finished = self.finish(false);
            if !playback.active {
                return if finished {
                    ListeningUpdate::Updated
                } else {
                    ListeningUpdate::Unchanged
                };
            }
            self.history.record_started(song.clone(), started_at());
            self.recent.insert(0, song.clone());
            self.recent.truncate(HISTORY_DISPLAY_LIMIT);
            self.active = Some(ActiveListening {
                source: song.source.clone(),
                occurrence,
                song_id: song.id.clone(),
                last_position: playback.position,
                last_seek_serial: playback.seek_serial,
                accumulated: Duration::ZERO,
                last_saved: now,
            });
            return ListeningUpdate::Started;
        }

        let active = self.active.as_mut().expect("observing an active song");
        if playback.active && !playback.paused && active.last_seek_serial == playback.seek_serial {
            active.accumulated += listening_position_delta(active.last_position, playback.position);
        }
        // Always track the observed position so resuming after a pause or a
        // restart does not count the intervening jump as time spent listening.
        active.last_position = playback.position;
        // A fast seek may finish entirely between polls, including a jump below
        // the ordinary two-second delta limit. Its completion resets the baseline.
        active.last_seek_serial = playback.seek_serial;
        if now.duration_since(active.last_saved) < PROGRESS_SAVE_INTERVAL {
            return ListeningUpdate::Unchanged;
        }
        active.update_history(&mut self.history, false);
        active.last_saved = now;
        ListeningUpdate::Updated
    }

    /// Natural ends complete the event; switches, stops, and exit keep it partial.
    pub(in crate::app) fn finish(&mut self, completed: bool) -> bool {
        let Some(active) = self.active.take() else {
            return false;
        };
        active.update_history(&mut self.history, completed);
        true
    }
}

/// Reject backward seeks and large forward jumps while allowing ordinary output.
fn listening_position_delta(previous: Duration, current: Duration) -> Duration {
    current
        .checked_sub(previous)
        .filter(|delta| *delta <= Duration::from_secs(2))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;
