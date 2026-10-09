//! Platform media controls. Commands are consumed on the UI thread so every
//! control surface shares the same queue, persistence, and playback behavior.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::Image;

use crate::{audio::PlaybackState, models::Song};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::MediaSession;

#[derive(Debug)]
pub enum MediaCommand {
    Play,
    Pause,
    PlayPause,
    Stop,
    Next,
    Previous,
    Seek(i64),
    SetPosition {
        track_id: String,
        position: Duration,
    },
    Volume(f32),
    Raise,
    Quit,
}

pub struct MediaUpdate<'a> {
    pub server: &'a str,
    pub song: Option<&'a Song>,
    pub cover: Option<&'a Arc<Image>>,
    pub playback: &'a PlaybackState,
    pub position: Duration,
    pub volume: f32,
    pub can_play: bool,
    pub can_next: bool,
    pub can_previous: bool,
}

pub fn track_id(server: &str, song_id: &str) -> String {
    use md5::{Digest, Md5};
    let mut hash = Md5::new();
    hash.update(server.as_bytes());
    hash.update([0]);
    hash.update(song_id.as_bytes());
    format!("/org/mpris/MediaPlayer2/track/t{:x}", hash.finalize())
}

pub fn seek_offset(position: Duration, offset_us: i64, duration: Option<Duration>) -> Duration {
    let offset = Duration::from_micros(offset_us.unsigned_abs());
    let target = if offset_us >= 0 {
        position.saturating_add(offset)
    } else {
        position.saturating_sub(offset)
    };
    duration.map_or(target, |limit| target.min(limit))
}

#[cfg(not(target_os = "linux"))]
pub struct MediaSession;

#[cfg(not(target_os = "linux"))]
impl MediaSession {
    pub fn new(_: &tokio::runtime::Handle) -> Self {
        Self
    }
    pub fn update(&mut self, _: MediaUpdate<'_>) {}
    pub fn restored_position_changed(&mut self) {}
    pub fn next_command(&self) -> Option<MediaCommand> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_paths_are_opaque_valid_and_server_specific() {
        let a = track_id("https://one", "../一首歌?token=secret");
        assert!(a
            .rsplit('/')
            .next()
            .unwrap()
            .bytes()
            .all(|b| b.is_ascii_alphanumeric()));
        assert!(!a.contains("secret"));
        assert_ne!(a, track_id("https://two", "../一首歌?token=secret"));
    }

    #[test]
    fn relative_seeking_handles_negative_and_extreme_offsets() {
        let position = Duration::from_secs(10);
        let duration = Some(Duration::from_secs(20));
        assert_eq!(
            seek_offset(position, -5_000_000, duration),
            Duration::from_secs(5)
        );
        assert_eq!(seek_offset(position, i64::MIN, duration), Duration::ZERO);
        assert_eq!(
            seek_offset(position, i64::MAX, duration),
            Duration::from_secs(20)
        );
    }
}
