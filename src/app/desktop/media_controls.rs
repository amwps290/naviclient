//! Publish player metadata and queue capabilities to the platform media session.

use super::media_command_state::MediaCapabilities;
use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn media_capabilities(&self) -> MediaCapabilities {
        MediaCapabilities::for_queue(
            self.playback_source.is_some(),
            self.state.queue_index,
            self.state.queue.len(),
            self.state.playback_modes,
            self.state.shuffle_history.played.len(),
        )
    }

    pub(in crate::app) fn sync_media_session(&mut self, playback: &PlaybackState) {
        let capabilities = self.media_capabilities();
        let song = self.state.now_playing.as_ref();
        let cover = song.and_then(|song| self.song_cover_image(song)).cloned();
        self.media_session.update(MediaUpdate {
            server: &self.playback_source_key(),
            song,
            occurrence: self.state.occurrence.id(),
            cover: cover.as_ref(),
            playback,
            position: self
                .session
                .timeline(playback, song)
                .map_or(Duration::ZERO, |timeline| timeline.position),
            volume: self.volume.output_volume(self.config.volume),
            can_play: capabilities.play,
            can_next: capabilities.next,
            can_previous: capabilities.previous,
        });
    }
}
