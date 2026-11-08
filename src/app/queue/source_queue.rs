//! Queues edited from another browsing source remain under that source's owner.

use crate::app::prelude::*;
use crate::source::SourceId;

pub(in crate::app) enum SourceQueueEdit {
    Append(QueueOrigin),
    InsertNext,
}

pub(in crate::app) fn edit_stored_queue(
    mut session: PlaybackSession,
    source: &SourceId,
    mut songs: Vec<Song>,
    edit: SourceQueueEdit,
) -> Option<PlaybackSession> {
    if session.source.as_ref().is_some_and(|owner| owner != source) {
        return None;
    }
    if songs.iter().any(|song| !song.belongs_to(source)) {
        return None;
    }
    for song in &mut songs {
        song.source = Some(source.clone());
    }
    let mut queue = PlaybackQueue::from_snapshot(session.queue, session.queue_origins);
    if !queue.bind_source(source) {
        return None;
    }
    let index = session.queue_index.filter(|index| *index < queue.len());
    if index.is_none() {
        session.position_secs = 0.0;
    }
    session.shuffle_played.retain(|index| *index < queue.len());
    session.shuffle_forward.retain(|index| *index < queue.len());
    match edit {
        SourceQueueEdit::Append(origin) => {
            queue.insert(queue.len(), songs, origin);
        }
        SourceQueueEdit::InsertNext => {
            let at = index.map_or(0, |index| index + 1);
            let count = songs.len();
            queue.insert(at, songs, QueueOrigin::Manual);
            for index in session
                .shuffle_played
                .iter_mut()
                .chain(session.shuffle_forward.iter_mut())
            {
                if *index >= at {
                    *index += count;
                }
            }
        }
    }
    let (songs, origins) = queue.snapshot();
    session.source = Some(source.clone());
    session.queue = songs;
    session.queue_origins = origins;
    session.queue_index = index.or_else(|| (!session.queue.is_empty()).then_some(0));
    Some(session)
}

impl NavidromeApp {
    /// Return bound songs for the current queue, or persist the edit to the
    /// inactive source's session without touching current audio or preloading.
    pub(in crate::app) fn route_queue_edit(
        &mut self,
        mut songs: Vec<Song>,
        edit: SourceQueueEdit,
    ) -> Option<Vec<Song>> {
        let song = songs.first()?;
        let api = self.source_for_song(song)?.clone();
        let source = api.id().clone();
        if songs.iter().any(|song| !song.belongs_to(&source)) {
            self.notify_error(t!("A queue can only contain tracks from one source"));
            return None;
        }
        for song in &mut songs {
            song.source = Some(source.clone());
        }
        if self.playback_source.is_none() {
            self.playback_source = Some(api.clone());
            self.load_playback_source_cache();
        }
        if self
            .playback_source
            .as_ref()
            .is_some_and(|playing| playing.id() == &source)
        {
            return Some(songs);
        }
        let session = self
            .server_cache_writer
            .pending_source_session(&source)
            .filter(|session| session.matches_source(&source, Some(api.base_url())))
            .or_else(|| config::load_source_session(&source, Some(api.base_url())))
            .unwrap_or_else(|| PlaybackSession {
                source: Some(source.clone()),
                server_url: api.base_url().to_string(),
                playback_modes: self.config.playback_modes,
                ..Default::default()
            });
        let Some(session) = edit_stored_queue(session, &source, songs, edit) else {
            self.notify_error(t!("A queue can only contain tracks from one source"));
            return None;
        };
        match self.server_cache_writer.submit_source(&source, session) {
            Ok(()) => self.notify_success(t!("Added to the selected source's queue")),
            Err(error) => self.notify_error(t!(
                "Config save failed: %{error}",
                error = error.to_string()
            )),
        }
        None
    }
}

#[cfg(test)]
mod tests;
