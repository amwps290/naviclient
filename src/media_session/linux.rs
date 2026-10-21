use std::collections::HashMap;
use std::sync::{mpsc, Arc, RwLock};
use std::time::Duration;

use gpui_kit::Image;
use rust_i18n::t;
use tokio::{runtime::Handle, sync::watch, task::JoinHandle};
use zbus::{
    fdo,
    object_server::SignalEmitter,
    zvariant::{ObjectPath, OwnedValue, Str, Value},
};

use super::{track_id, MediaCommand, MediaUpdate};

const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

#[derive(Clone)]
struct Track {
    server: String,
    song_id: String,
    path: String,
    title: String,
    artist: String,
    album: String,
    length: i64,
    cover: Option<Arc<Image>>,
}

#[derive(Clone)]
struct Snapshot {
    track: Option<Arc<Track>>,
    status: &'static str,
    position: i64,
    seek_serial: u64,
    volume: f64,
    can_play: bool,
    can_pause: bool,
    can_seek: bool,
    can_next: bool,
    can_previous: bool,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            track: None,
            status: "Stopped",
            position: 0,
            seek_serial: 0,
            volume: 0.0,
            can_play: false,
            can_pause: false,
            can_seek: false,
            can_next: false,
            can_previous: false,
        }
    }
}

fn micros(value: Duration) -> i64 {
    value.as_micros().min(i64::MAX as u128) as i64
}

fn same_track(a: &Option<Arc<Track>>, b: &Option<Arc<Track>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}

fn needs_notification(a: &Snapshot, b: &Snapshot) -> bool {
    !same_track(&a.track, &b.track)
        || a.status != b.status
        || a.volume != b.volume
        || a.can_play != b.can_play
        || a.can_pause != b.can_pause
        || a.can_seek != b.can_seek
        || a.can_next != b.can_next
        || a.can_previous != b.can_previous
        || a.seek_serial != b.seek_serial
}

#[derive(Clone)]
struct Bridge {
    state: Arc<RwLock<Snapshot>>,
    art: Arc<RwLock<Option<(u64, String)>>>,
    commands: mpsc::SyncSender<MediaCommand>,
}

impl Bridge {
    fn snapshot(&self) -> Snapshot {
        self.state.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn send(&self, command: MediaCommand) -> fdo::Result<()> {
        self.commands
            .try_send(command)
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => {
                    fdo::Error::LimitsExceeded("Media command queue is full".into())
                }
                mpsc::TrySendError::Disconnected(_) => {
                    fdo::Error::Failed("Player is closing".into())
                }
            })
    }

    fn metadata(&self) -> HashMap<String, OwnedValue> {
        let mut metadata = HashMap::new();
        let Some(track) = self.snapshot().track else {
            return metadata;
        };
        metadata.insert(
            "mpris:trackid".into(),
            ObjectPath::try_from(track.path.as_str())
                .expect("generated track path")
                .into(),
        );
        metadata.insert("mpris:length".into(), track.length.into());
        metadata.insert("xesam:title".into(), Str::from(track.title.as_str()).into());
        metadata.insert("xesam:album".into(), Str::from(track.album.as_str()).into());
        metadata.insert(
            "xesam:artist".into(),
            Value::from(vec![track.artist.as_str()])
                .try_into()
                .expect("string array"),
        );
        let art = self.art.read().unwrap_or_else(|e| e.into_inner());
        if let Some((_, url)) = art
            .as_ref()
            .filter(|(id, _)| track.cover.as_ref().is_some_and(|cover| cover.id == *id))
        {
            metadata.insert("mpris:artUrl".into(), Str::from(url.as_str()).into());
        }
        metadata
    }
}

pub struct MediaSession {
    bridge: Bridge,
    commands: mpsc::Receiver<MediaCommand>,
    wake: watch::Sender<()>,
    task: JoinHandle<()>,
    restored_seek_serial: u64,
}

impl MediaSession {
    pub fn new(runtime: &Handle) -> Self {
        let (tx, commands) = mpsc::sync_channel(32);
        let bridge = Bridge {
            state: Arc::new(RwLock::new(Snapshot::default())),
            art: Arc::new(RwLock::new(None)),
            commands: tx,
        };
        let (wake, updates) = watch::channel(());
        let worker = bridge.clone();
        let task = runtime.spawn(async move {
            if let Err(error) = serve(worker, updates).await {
                log::warn!("Linux media session unavailable (playback is unaffected): {error}");
            }
        });
        Self {
            bridge,
            commands,
            wake,
            task,
            restored_seek_serial: 0,
        }
    }

    pub fn next_command(&self) -> Option<MediaCommand> {
        self.commands.try_recv().ok()
    }

    pub fn restored_position_changed(&mut self) {
        self.restored_seek_serial = self.restored_seek_serial.wrapping_add(1);
    }

    pub fn update(&mut self, update: MediaUpdate<'_>) {
        if self.wake.is_closed() {
            return;
        }
        let mut state = self.bridge.state.write().unwrap_or_else(|e| e.into_inner());
        let track = update.song.map(|song| {
            let length = if update.playback.active {
                update.playback.duration
            } else {
                None
            }
            .unwrap_or_else(|| {
                Duration::from_secs(song.duration.unwrap_or_default().max(0) as u64)
            });
            let cover_id = update.cover.map(|cover| cover.id);
            if let Some(track) = state.track.as_ref().filter(|track| {
                track.server == update.server
                    && track.song_id == song.id
                    && track.title == song.title
                    && track.artist == song.artist
                    && track.album == song.album
                    && track.length == micros(length)
                    && track.cover.as_ref().map(|cover| cover.id) == cover_id
            }) {
                return track.clone();
            }
            Arc::new(Track {
                server: update.server.into(),
                song_id: song.id.clone(),
                path: track_id(update.server, &song.id),
                title: song.title.clone(),
                artist: song.artist.clone(),
                album: song.album.clone(),
                length: micros(length),
                cover: update.cover.cloned(),
            })
        });
        let status = if track.is_none() || update.playback.ended || update.playback.error.is_some()
        {
            "Stopped"
        } else if update.playback.active && !update.playback.paused {
            "Playing"
        } else {
            "Paused"
        };
        let next = Snapshot {
            track,
            status,
            position: micros(update.position),
            seek_serial: update
                .playback
                .seek_serial
                .wrapping_add(self.restored_seek_serial),
            volume: update.volume as f64,
            can_play: update.can_play,
            can_pause: update.playback.active && !update.playback.ended,
            can_seek: update.song.is_some() && update.can_play,
            can_next: update.can_next,
            can_previous: update.can_previous,
        };
        let notify = needs_notification(&state, &next);
        *state = next;
        drop(state);
        // Position is cheap shared state, read on demand. MPRIS forbids periodic
        // PropertiesChanged for Position; ordinary playback wakes no D-Bus task.
        if notify {
            let _ = self.wake.send(());
        }
    }
}

impl Drop for MediaSession {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct RootInterface(Bridge);

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

struct PlayerInterface(Bridge);

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
        self.0.snapshot().status
    }
    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, OwnedValue> {
        self.0.metadata()
    }
    #[zbus(property(emits_changed_signal = "false"))]
    fn position(&self) -> i64 {
        self.0.snapshot().position
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
    async fn seeked(emitter: &SignalEmitter<'_>, position: i64) -> zbus::Result<()>;
}

async fn serve(bridge: Bridge, mut updates: watch::Receiver<()>) -> anyhow::Result<()> {
    let name = format!(
        "org.mpris.MediaPlayer2.navidrome_client.instance{}",
        std::process::id()
    );
    let connection = tokio::time::timeout(Duration::from_secs(3), async {
        zbus::connection::Builder::session()?
            .name(name.as_str())?
            .serve_at(PATH, RootInterface(bridge.clone()))?
            .serve_at(PATH, PlayerInterface(bridge.clone()))?
            .build()
            .await
    })
    .await??;
    log::info!("Linux MPRIS media session registered: {name}");
    // The directory and its single current image belong to this connection.
    let art_dir = tempfile::Builder::new()
        .prefix("navidrome-media-art-")
        .tempdir()
        .ok();
    let mut art_file = None;
    let mut previous = Snapshot::default();
    loop {
        let snapshot = bridge.snapshot();
        if !same_track(&previous.track, &snapshot.track) {
            let cover = snapshot
                .track
                .as_ref()
                .and_then(|track| track.cover.clone());
            let current_id = bridge
                .art
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .map(|(id, _)| *id);
            if current_id != cover.as_ref().map(|cover| cover.id) {
                let file = if let (Some(dir), Some(cover)) = (&art_dir, cover) {
                    let dir = dir.path().to_owned();
                    tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
                        let format = image::guess_format(&cover.bytes)?;
                        let extension = format.extensions_str().first().copied().unwrap_or("img");
                        let path = dir.join(format!("{:016x}.{extension}", cover.id));
                        std::fs::write(&path, &cover.bytes)?;
                        let url = url::Url::from_file_path(&path)
                            .map_err(|_| anyhow::anyhow!("invalid artwork path"))?;
                        Ok((cover.id, path, url.to_string()))
                    })
                    .await?
                    .map(Some)
                    .unwrap_or_else(|error| {
                        log::warn!("media artwork unavailable: {error}");
                        None
                    })
                } else {
                    None
                };
                *bridge.art.write().unwrap_or_else(|e| e.into_inner()) =
                    file.as_ref().map(|(id, _, url)| (*id, url.clone()));
                if let Some(path) = art_file.take() {
                    let _ = std::fs::remove_file(path);
                }
                art_file = file.map(|(_, path, _)| path);
            }
        }
        let mut changed: HashMap<&str, Value<'_>> = HashMap::new();
        if !same_track(&previous.track, &snapshot.track) {
            changed.insert("Metadata", Value::from(bridge.metadata()));
        }
        if previous.status != snapshot.status {
            changed.insert("PlaybackStatus", snapshot.status.into());
        }
        if previous.volume != snapshot.volume {
            changed.insert("Volume", snapshot.volume.into());
        }
        for (name, before, after) in [
            ("CanPlay", previous.can_play, snapshot.can_play),
            ("CanPause", previous.can_pause, snapshot.can_pause),
            ("CanSeek", previous.can_seek, snapshot.can_seek),
            ("CanGoNext", previous.can_next, snapshot.can_next),
            (
                "CanGoPrevious",
                previous.can_previous,
                snapshot.can_previous,
            ),
        ] {
            if before != after {
                changed.insert(name, after.into());
            }
        }
        if !changed.is_empty() {
            connection
                .emit_signal(
                    None::<&str>,
                    PATH,
                    "org.freedesktop.DBus.Properties",
                    "PropertiesChanged",
                    &(PLAYER, changed, Vec::<String>::new()),
                )
                .await?;
        }
        if previous.seek_serial != snapshot.seek_serial {
            PlayerInterface::seeked(&SignalEmitter::new(&connection, PATH)?, snapshot.position)
                .await?;
        }
        previous = snapshot;
        tokio::select! {
            result = updates.changed() => if result.is_err() { break; },
            _ = connection.closed() => break,
        }
    }
    connection.close().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bridge() -> (Bridge, mpsc::Receiver<MediaCommand>) {
        let (commands, rx) = mpsc::sync_channel(2);
        (
            Bridge {
                state: Arc::new(RwLock::new(Snapshot::default())),
                art: Arc::new(RwLock::new(None)),
                commands,
            },
            rx,
        )
    }

    #[test]
    fn ordinary_position_updates_do_not_emit_property_signals() {
        let before = Snapshot::default();
        let mut after = before.clone();
        after.position = 500_000;
        assert!(!needs_notification(&before, &after));
        after.seek_serial = 1;
        assert!(needs_notification(&before, &after));
        after.seek_serial = 0;
        after.status = "Playing";
        assert!(needs_notification(&before, &after));
    }

    #[test]
    fn commands_are_bounded_and_volume_rejects_nonfinite_values() {
        let (bridge, rx) = bridge();
        let player = PlayerInterface(bridge);
        assert!(player.set_volume(f64::NAN).is_err());
        assert!(rx.try_recv().is_err());
        player.set_volume(2.0).unwrap();
        assert!(matches!(rx.recv().unwrap(), MediaCommand::Volume(v) if v == 1.0));
        player.play().unwrap();
        player.pause().unwrap();
        assert!(matches!(player.next(), Err(fdo::Error::LimitsExceeded(_))));
    }

    #[test]
    fn negative_absolute_positions_are_ignored_and_rate_is_fixed() {
        let (bridge, rx) = bridge();
        let player = PlayerInterface(bridge);
        player
            .set_position(ObjectPath::try_from("/track").unwrap(), -1)
            .unwrap();
        assert!(rx.try_recv().is_err());
        assert!(player.set_rate(1.0).is_ok());
        assert!(player.set_rate(1.5).is_err());
    }

    #[test]
    fn metadata_uses_object_paths_and_never_publishes_server_urls() {
        let (bridge, _) = bridge();
        bridge.state.write().unwrap().track = Some(Arc::new(Track {
            server: "https://user:secret@example.test".into(),
            song_id: "unsafe/id".into(),
            path: track_id("https://user:secret@example.test", "unsafe/id"),
            title: "Track".into(),
            artist: "Artist".into(),
            album: "Album".into(),
            length: 42_000_000,
            cover: None,
        }));
        let metadata = bridge.metadata();
        assert!(matches!(&*metadata["mpris:trackid"], Value::ObjectPath(_)));
        assert_eq!(
            i64::try_from(&metadata["mpris:length"]).unwrap(),
            42_000_000
        );
        assert!(!metadata.contains_key("xesam:url"));
        assert!(!format!("{metadata:?}").contains("secret"));
    }
}
