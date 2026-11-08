//! Playback session identity, pending resume position, restoration, and save throttling.

use std::time::{Duration, Instant};

use crate::app::queue::logic::ShuffleHistory;
use crate::app::shell::state::AppState;
use crate::app::songs::scope::SongScope;
use crate::audio::PlaybackState;
use crate::models::{PlayModes, PlaybackQueue, PlaybackSession, Song};
use crate::playback_timeline::PlaybackTimeline;
use crate::source::{ProviderKind, SourceId};

const SESSION_SAVE_INTERVAL: Duration = Duration::from_secs(5);

struct SessionServer {
    id: Option<String>,
    url: String,
    source: Option<SourceId>,
}

pub(in crate::app) struct SessionState {
    server: Option<SessionServer>,
    resume_position: Option<Duration>,
    last_saved: Instant,
}

impl SessionState {
    pub(in crate::app) fn new(now: Instant) -> Self {
        Self {
            server: None,
            resume_position: None,
            last_saved: now,
        }
    }

    pub(in crate::app) fn bind(&mut self, server_id: Option<&str>, server_url: Option<&str>) {
        self.server = server_url.map(|url| SessionServer {
            id: server_id.map(str::to_string),
            url: url.to_string(),
            source: None,
        });
        self.clear_position();
    }

    pub(in crate::app) fn server_id(&self) -> Option<&str> {
        self.server.as_ref().and_then(|server| server.id.as_deref())
    }

    pub(in crate::app) fn bind_source(&mut self, source: SourceId, server_url: Option<&str>) {
        let id = match source.provider {
            ProviderKind::OpenSubsonic => source.profile_id.clone(),
            ProviderKind::Bilibili => source.cache_namespace(),
        };
        self.server = Some(SessionServer {
            id: Some(id),
            url: server_url.unwrap_or_default().to_string(),
            source: Some(source),
        });
        self.clear_position();
    }

    pub(in crate::app) fn source(&self) -> Option<&SourceId> {
        self.server.as_ref()?.source.as_ref()
    }

    pub(in crate::app) fn clear_position(&mut self) {
        self.resume_position = None;
    }

    pub(in crate::app) fn set_position(&mut self, timeline: PlaybackTimeline) {
        self.resume_position = Some(timeline.with_position(timeline.position).position);
    }

    pub(in crate::app) fn position(&self, observed: Duration) -> Duration {
        self.resume_position.unwrap_or(observed)
    }

    /// Play resumes a saved position, otherwise starts from zero. A terminal
    /// worker position belongs to retry/display and must not restart at the end.
    pub(in crate::app) fn start_position(&self, song: Option<&Song>) -> Duration {
        self.timeline(&PlaybackState::default(), song)
            .map_or(Duration::ZERO, |timeline| timeline.position)
    }

    pub(in crate::app) fn display_playback(
        &self,
        mut playback: PlaybackState,
        song: Option<&Song>,
    ) -> PlaybackState {
        let timeline = self.timeline(&playback, song).unwrap_or_default();
        playback.position = timeline.position;
        playback.duration = timeline.duration;
        playback
    }

    pub(in crate::app) fn timeline(
        &self,
        playback: &PlaybackState,
        song: Option<&Song>,
    ) -> Option<PlaybackTimeline> {
        let song = song?;
        let timeline = PlaybackTimeline::for_song(playback, song);
        Some(timeline.with_position(self.position(timeline.position)))
    }

    pub(in crate::app) fn restore(
        &mut self,
        state: &mut AppState,
        mut session: PlaybackSession,
    ) -> bool {
        if self
            .server
            .as_ref()
            .is_none_or(|server| match server.source.as_ref() {
                Some(source) => !session.matches_source(source, Some(&server.url)),
                None => !session.matches_server(&server.url),
            })
            || session.queue.is_empty()
        {
            return false;
        }
        if let Some(source) = self.source() {
            if session.queue.iter().any(|song| !song.belongs_to(source)) {
                return false;
            }
            for song in &mut session.queue {
                song.source = Some(source.clone());
            }
        }
        let restored = RestoredSession::prepare(session);
        self.resume_position = restored.apply(state);
        true
    }

    pub(in crate::app) fn snapshot(
        &self,
        state: &AppState,
        playback: &PlaybackState,
    ) -> Option<PlaybackSession> {
        let server = self.server.as_ref()?;
        let (mut queue, queue_origins) = state.queue.snapshot();
        if let Some(source) = self.source() {
            if queue.iter().any(|song| !song.belongs_to(source)) {
                return None;
            }
            for song in &mut queue {
                song.source = Some(source.clone());
            }
        }
        let (shuffle_played, shuffle_forward) = state.shuffle_history.snapshot();
        Some(PlaybackSession {
            source: server.source.clone(),
            server_url: server.url.clone(),
            queue,
            queue_origins,
            queue_index: state.queue_index,
            position_secs: self
                .timeline(playback, state.now_playing.as_ref())
                .map_or(Duration::ZERO, |timeline| timeline.position)
                .as_secs_f64(),
            playback_modes: state.playback_modes,
            shuffle_played,
            shuffle_forward,
        })
    }

    pub(in crate::app) fn submitted(&mut self, now: Instant) {
        self.last_saved = now;
    }

    pub(in crate::app) fn should_save(&self, now: Instant, has_queue: bool) -> bool {
        has_queue && now.duration_since(self.last_saved) >= SESSION_SAVE_INTERVAL
    }
}

/// Queue identities, selected song, history and resume clock are prepared before
/// changing application state; applying an accepted session cannot fail midway.
struct RestoredSession {
    queue: PlaybackQueue,
    index: Option<usize>,
    history: ShuffleHistory,
    modes: PlayModes,
    position: Option<Duration>,
}

impl RestoredSession {
    fn prepare(session: PlaybackSession) -> Self {
        let queue = PlaybackQueue::from_snapshot(session.queue, session.queue_origins);
        let index = session.queue_index.filter(|index| *index < queue.len());
        let mut history = ShuffleHistory::restore(session.shuffle_played, session.shuffle_forward);
        history.remap(|index| (index < queue.len()).then_some(index));
        let position = index.and_then(|index| queue.song(index)).map(|song| {
            PlaybackTimeline::for_song(&PlaybackState::default(), song)
                .with_position(restored_position(session.position_secs))
                .position
        });
        Self {
            queue,
            index,
            history,
            modes: session.playback_modes,
            position,
        }
    }

    fn apply(self, state: &mut AppState) -> Option<Duration> {
        state.song_selection.clear_scope(SongScope::Queue);
        state.queue = self.queue;
        state.queue_index = self.index;
        state.playback_modes = self.modes;
        state.shuffle_history = self.history;
        state.now_playing = self
            .index
            .and_then(|index| state.queue.song(index))
            .cloned();
        state.now_playing_quality = None;
        state.occurrence.clear();
        if let Some((entry, song)) = self
            .index
            .and_then(|index| state.queue.entry_id(index).zip(state.queue.song(index)))
        {
            state.occurrence.restore(entry, &song.id);
        }
        self.position
    }
}

fn restored_position(seconds: f64) -> Duration {
    Duration::try_from_secs_f64(seconds.max(0.0)).unwrap_or_default()
}

#[cfg(test)]
mod tests;
