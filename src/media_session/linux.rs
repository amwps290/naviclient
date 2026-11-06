//! Linux media-session bridge, connection lifetime, and snapshot signal publication.

use std::collections::HashMap;
use std::sync::{mpsc, Arc, RwLock};
use std::time::Duration;

use tokio::{runtime::Handle, sync::watch, task::JoinHandle};
use zbus::{
    fdo,
    object_server::SignalEmitter,
    zvariant::{ObjectPath, OwnedValue, Str, Value},
};

use super::state::{micros, Snapshot};
use super::{MediaCommand, MediaUpdate};

mod artwork;
mod interfaces;

use artwork::Artwork;
use interfaces::{PlayerInterface, RootInterface};

const PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";

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
        self.metadata_for(&self.snapshot())
    }

    fn metadata_for(&self, snapshot: &Snapshot) -> HashMap<String, OwnedValue> {
        let mut metadata = HashMap::new();
        let Some(track) = snapshot.track.as_ref() else {
            return metadata;
        };
        metadata.insert(
            "mpris:trackid".into(),
            ObjectPath::try_from(track.path.as_str())
                .expect("generated track path")
                .into(),
        );
        metadata.insert("mpris:length".into(), micros(track.duration).into());
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
    pub fn new(runtime: &Handle, _window_handle: Option<isize>) -> Self {
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
        let next = Snapshot::project(state.track.as_ref(), &update, self.restored_seek_serial);
        let notify = next.changes_from(&state).any();
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
    let mut artwork = Artwork::new(bridge.art.clone());
    let mut previous = Snapshot::default();
    loop {
        let snapshot = bridge.snapshot();
        let changes = snapshot.changes_from(&previous);
        if changes.metadata {
            let cover = snapshot
                .track
                .as_ref()
                .and_then(|track| track.cover.clone());
            artwork.update(cover).await?;
        }
        let mut changed: HashMap<&str, Value<'_>> = HashMap::new();
        if changes.metadata {
            // Artwork I/O can await while the UI advances to another song.
            // Publish metadata from the same snapshot as status/capabilities.
            changed.insert("Metadata", Value::from(bridge.metadata_for(&snapshot)));
        }
        if changes.status {
            changed.insert("PlaybackStatus", snapshot.status.as_mpris().into());
        }
        if changes.volume {
            changed.insert("Volume", snapshot.volume.into());
        }
        for (name, did_change, value) in [
            ("CanPlay", changes.can_play, snapshot.can_play),
            ("CanPause", changes.can_pause, snapshot.can_pause),
            ("CanSeek", changes.can_seek, snapshot.can_seek),
            ("CanGoNext", changes.can_next, snapshot.can_next),
            ("CanGoPrevious", changes.can_previous, snapshot.can_previous),
        ] {
            if did_change {
                changed.insert(name, value.into());
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
        if changes.seeked {
            PlayerInterface::seeked(
                &SignalEmitter::new(&connection, PATH)?,
                micros(snapshot.position),
            )
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
