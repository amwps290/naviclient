use std::sync::{mpsc, Arc, RwLock};
use std::time::Duration;

use zbus::{
    fdo,
    zvariant::{ObjectPath, Value},
};

use super::PlayerInterface;
use crate::media_session::linux::Bridge;
use crate::media_session::state::Snapshot;
use crate::media_session::{MediaCommand, MediaUpdate};

#[test]
fn introspection_preserves_mpris_property_and_seek_signatures() {
    use zbus::object_server::Interface;

    use super::RootInterface;

    let (bridge, _) = bridge();
    let mut root = String::new();
    RootInterface(bridge.clone()).introspect_to_writer(&mut root, 0);
    assert!(root.contains("interface name=\"org.mpris.MediaPlayer2\""));
    assert!(root.contains("method name=\"Raise\""));
    assert!(root.contains("method name=\"Quit\""));

    let mut player = String::new();
    PlayerInterface(bridge).introspect_to_writer(&mut player, 0);
    assert!(player.contains("interface name=\"org.mpris.MediaPlayer2.Player\""));
    let section = |name: &str, end: &str| {
        let start = player.find(name).unwrap();
        let end = start + player[start..].find(end).unwrap();
        &player[start..end]
    };
    let position = section("property name=\"Position\"", "</property>");
    assert!(position.contains("type=\"x\""));
    assert!(position.contains("access=\"read\""));
    assert!(position.contains("org.freedesktop.DBus.Property.EmitsChangedSignal"));
    assert!(position.contains("value=\"false\""));
    let metadata = section("property name=\"Metadata\"", "</property>");
    assert!(metadata.contains("type=\"a{sv}\""));
    let seek = section("method name=\"SetPosition\"", "</method>");
    assert!(seek.contains("type=\"o\""));
    assert!(seek.contains("type=\"x\""));
    let seeked = section("signal name=\"Seeked\"", "</signal>");
    assert!(seeked.contains("type=\"x\""));
}
use crate::{audio::PlaybackState, models::Song};
use gpui_kit::{Image, ImageFormat};

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

fn snapshot(title: &str, cover: Option<&Arc<Image>>) -> Snapshot {
    let song = Song {
        id: title.into(),
        title: title.into(),
        artist: "Artist".into(),
        album: "Album".into(),
        duration: Some(42),
        ..Song::default()
    };
    Snapshot::project(
        None,
        &MediaUpdate {
            server: "https://user:secret@example.test",
            occurrence: None,
            song: Some(&song),
            cover,
            playback: &PlaybackState::default(),
            position: Duration::ZERO,
            volume: 0.5,
            can_play: true,
            can_next: true,
            can_previous: false,
        },
        0,
    )
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
fn rejected_volume_commands_preserve_advertised_volume_and_pending_command_order() {
    let (bridge, rx) = bridge();
    let player = PlayerInterface(bridge);
    player.set_volume(0.4).unwrap();
    assert!(matches!(rx.recv().unwrap(), MediaCommand::Volume(_)));
    let volume = player.volume();
    player.play().unwrap();
    player.pause().unwrap();

    assert!(matches!(
        player.set_volume(0.9),
        Err(fdo::Error::LimitsExceeded(_))
    ));
    assert_eq!(player.volume(), volume);
    assert!(matches!(rx.recv().unwrap(), MediaCommand::Play));
    assert!(matches!(rx.recv().unwrap(), MediaCommand::Pause));
    drop(rx);
    assert!(matches!(player.set_volume(0.8), Err(fdo::Error::Failed(_))));
    assert_eq!(player.volume(), volume);
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
    *bridge.state.write().unwrap() = snapshot("unsafe/id", None);
    let metadata = bridge.metadata();
    assert!(matches!(&*metadata["mpris:trackid"], Value::ObjectPath(_)));
    assert_eq!(
        i64::try_from(&metadata["mpris:length"]).unwrap(),
        42_000_000
    );
    assert!(!metadata.contains_key("xesam:url"));
    assert!(!format!("{metadata:?}").contains("secret"));
}

#[test]
fn queued_metadata_uses_the_captured_track_after_the_ui_switches_songs() {
    let (bridge, _) = bridge();
    *bridge.state.write().unwrap() = snapshot("first", None);
    let captured = bridge.snapshot();
    *bridge.state.write().unwrap() = snapshot("second", None);
    let signal = bridge.metadata_for(&captured);
    let current = bridge.metadata();
    assert_eq!(<&str>::try_from(&signal["xesam:title"]).unwrap(), "first");
    assert_eq!(<&str>::try_from(&current["xesam:title"]).unwrap(), "second");
    assert_ne!(signal["mpris:trackid"], current["mpris:trackid"]);
}

#[test]
fn artwork_is_published_only_for_the_matching_snapshot_cover() {
    let (bridge, _) = bridge();
    let cover = Arc::new(Image::from_bytes(
        ImageFormat::Png,
        include_bytes!("../../../../assets/default-cover.png").to_vec(),
    ));
    let captured = snapshot("first", Some(&cover));
    *bridge.state.write().unwrap() = snapshot("second", None);
    *bridge.art.write().unwrap() = Some((cover.id, "file:///tmp/current-cover.png".into()));
    let signal = bridge.metadata_for(&captured);
    assert_eq!(
        <&str>::try_from(&signal["mpris:artUrl"]).unwrap(),
        "file:///tmp/current-cover.png"
    );
    assert!(!bridge.metadata().contains_key("mpris:artUrl"));
    *bridge.art.write().unwrap() = Some((cover.id.wrapping_add(1), "file:///tmp/stale.png".into()));
    assert!(!bridge.metadata_for(&captured).contains_key("mpris:artUrl"));
}
