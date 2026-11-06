//! Desktop notification eligibility and tray updates, without system calls.

use crate::audio::PlaybackState;

#[derive(Debug, PartialEq, Eq)]
pub(in crate::app) enum TrackNotificationUpdate {
    Unchanged,
    CancelPending,
    Send,
}

struct TrackIdentity {
    server_id: Option<String>,
    song_id: String,
}

#[derive(Default)]
pub(in crate::app) struct DesktopState {
    last_notified_track: Option<TrackIdentity>,
    last_tray_paused: Option<bool>,
}

impl DesktopState {
    /// Record a notification attempt only when the audio has connected.
    /// Pausing/resuming or repeating the same song does not announce it again.
    pub(in crate::app) fn track_notification_update(
        &mut self,
        enabled: bool,
        audio_active: bool,
        server_id: Option<&str>,
        song_id: Option<&str>,
    ) -> TrackNotificationUpdate {
        let Some(song_id) = song_id else {
            self.reset_track();
            return TrackNotificationUpdate::CancelPending;
        };
        if !enabled || !audio_active {
            return TrackNotificationUpdate::CancelPending;
        }
        if self.last_notified_track.as_ref().is_some_and(|track| {
            track.server_id.as_deref() == server_id && track.song_id == song_id
        }) {
            return TrackNotificationUpdate::Unchanged;
        }
        self.last_notified_track = Some(TrackIdentity {
            server_id: server_id.map(str::to_owned),
            song_id: song_id.to_owned(),
        });
        TrackNotificationUpdate::Send
    }

    /// Reconnecting even the same server starts a fresh playback context.
    pub(in crate::app) fn reset_track(&mut self) {
        self.last_notified_track = None;
    }

    pub(in crate::app) fn tray_paused_update(&mut self, playback: &PlaybackState) -> Option<bool> {
        let paused = !playback.active || playback.paused;
        if self.last_tray_paused == Some(paused) {
            return None;
        }
        self.last_tray_paused = Some(paused);
        Some(paused)
    }
}

#[cfg(test)]
mod tests;
