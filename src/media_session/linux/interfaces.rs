//! MPRIS root/player interfaces and typed media-command validation.

use std::collections::HashMap;
use std::time::Duration;

use rust_i18n::t;
use zbus::{
    fdo,
    object_server::SignalEmitter,
    zvariant::{ObjectPath, OwnedValue},
};

use super::Bridge;
use crate::media_session::state::micros;
use crate::media_session::MediaCommand;

pub(super) struct RootInterface(pub(super) Bridge);

#[zbus::interface(name = "org.mpris.MediaPlayer2")]
impl RootInterface {
    fn raise(&self) -> fdo::Result<()> {
        self.0.send(MediaCommand::Raise)
    }
    fn quit(&self) -> fdo::Result<()> {
        self.0.send(MediaCommand::Quit)
    }
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn can_raise(&self) -> bool {
        true
    }
    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }
    #[zbus(property)]
    fn identity(&self) -> &str {
        "Navidrome Client"
    }
    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<&str> {
        vec![]
    }
    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<&str> {
        vec![]
    }
}

pub(super) struct PlayerInterface(pub(super) Bridge);

#[zbus::interface(name = "org.mpris.MediaPlayer2.Player")]
impl PlayerInterface {
    fn next(&self) -> fdo::Result<()> {
        self.0.send(MediaCommand::Next)
    }
    fn previous(&self) -> fdo::Result<()> {
        self.0.send(MediaCommand::Previous)
    }
    fn pause(&self) -> fdo::Result<()> {
        self.0.send(MediaCommand::Pause)
    }
    fn play_pause(&self) -> fdo::Result<()> {
        self.0.send(MediaCommand::PlayPause)
    }
    fn stop(&self) -> fdo::Result<()> {
        self.0.send(MediaCommand::Stop)
    }
    fn play(&self) -> fdo::Result<()> {
        self.0.send(MediaCommand::Play)
    }
    fn seek(&self, offset: i64) -> fdo::Result<()> {
        self.0.send(MediaCommand::Seek(offset))
    }
    fn set_position(&self, track_id: ObjectPath<'_>, position: i64) -> fdo::Result<()> {
        if position < 0 {
            return Ok(());
        }
        self.0.send(MediaCommand::SetPosition {
            track_id: track_id.to_string(),
            position: Duration::from_micros(position as u64),
        })
    }
    fn open_uri(&self, _uri: &str) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported(
            t!("Choose a song from the library").to_string(),
        ))
    }
    #[zbus(property)]
    fn playback_status(&self) -> &str {
        self.0.snapshot().status.as_mpris()
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        self.0.metadata()
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        micros(self.0.snapshot().position)
    }
    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.0.snapshot().volume
    }
    #[zbus(property)]
    fn set_volume(&self, value: f64) -> fdo::Result<()> {
        if !value.is_finite() {
            return Err(fdo::Error::InvalidArgs("Volume must be finite".into()));
        }
        let value = value.clamp(0.0, 1.0) as f32;
        self.0.send(MediaCommand::Volume(value))?;
        self.0
            .state
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .volume = value as f64;
        Ok(())
    }
    #[zbus(property)]
    fn rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn set_rate(&self, value: f64) -> fdo::Result<()> {
        if value == 1.0 {
            Ok(())
        } else {
            Err(fdo::Error::NotSupported(
                "Only normal playback speed is supported".into(),
            ))
        }
    }
    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        1.0
    }
    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        self.0.snapshot().can_next
    }
    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        self.0.snapshot().can_previous
    }
    #[zbus(property)]
    fn can_play(&self) -> bool {
        self.0.snapshot().can_play
    }
    #[zbus(property)]
    fn can_pause(&self) -> bool {
        self.0.snapshot().can_pause
    }
    #[zbus(property)]
    fn can_seek(&self) -> bool {
        self.0.snapshot().can_seek
    }
    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }
    #[zbus(signal)]
    pub(super) async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;
}

#[cfg(test)]
mod tests;
