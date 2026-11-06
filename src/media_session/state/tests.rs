use super::*;
use crate::{audio::PlaybackState, models::Song};
use gpui_kit::ImageFormat;

fn song() -> Song {
    Song {
        id: "song".into(),
        title: "Title".into(),
        artist: "Artist".into(),
        album: "Album".into(),
        duration: Some(42),
        ..Song::default()
    }
}

fn update<'a>(song: Option<&'a Song>, playback: &'a PlaybackState) -> MediaUpdate<'a> {
    MediaUpdate {
        server: "https://server.test",
        occurrence: None,
        song,
        cover: None,
        playback,
        position: playback.position,
        volume: 0.5,
        can_play: true,
        can_next: true,
        can_previous: false,
    }
}

fn cover(marker: u8) -> Arc<Image> {
    let mut bytes = include_bytes!("../../../assets/default-cover.png").to_vec();
    bytes.push(marker);
    Arc::new(Image::from_bytes(ImageFormat::Png, bytes))
}

#[test]
fn restored_track_is_paused_seekable_and_uses_the_restored_position() {
    let song = song();
    let playback = PlaybackState::default();
    let mut update = update(Some(&song), &playback);
    update.position = Duration::from_secs(12);
    let snapshot = Snapshot::project(None, &update, 0);
    assert_eq!(snapshot.status, PlaybackStatus::Paused);
    assert_eq!(snapshot.position, Duration::from_secs(12));
    assert_eq!(snapshot.track.unwrap().duration, Duration::from_secs(42));
    assert!(snapshot.can_play && snapshot.can_seek);
    assert!(!snapshot.can_pause);
}

#[test]
fn repeated_song_changes_media_identity_and_retry_reuses_it() {
    let song = song();
    let playback = PlaybackState {
        active: true,
        ..Default::default()
    };
    let mut update = update(Some(&song), &playback);
    update.occurrence = Some(crate::models::PlaybackId::new());
    let first = Snapshot::project(None, &update, 0);
    let retry = Snapshot::project(first.track.as_ref(), &update, 0);
    assert!(same_track(&first.track, &retry.track));
    assert!(!retry.changes_from(&first).metadata);
    update.occurrence = Some(crate::models::PlaybackId::new());
    let replay = Snapshot::project(first.track.as_ref(), &update, 0);
    assert!(!same_track(&first.track, &replay.track));
    assert!(replay.changes_from(&first).metadata);
    assert_ne!(first.track.unwrap().path, replay.track.unwrap().path);
}

#[test]
fn idle_audio_duration_does_not_leak_from_a_previous_song() {
    let song = song();
    let playback = PlaybackState {
        duration: Some(Duration::from_secs(999)),
        ..PlaybackState::default()
    };
    let snapshot = Snapshot::project(None, &update(Some(&song), &playback), 0);
    assert_eq!(snapshot.track.unwrap().duration, Duration::from_secs(42));
}

#[test]
fn active_stream_duration_takes_precedence_over_song_metadata() {
    let song = song();
    let playback = PlaybackState {
        active: true,
        duration: Some(Duration::from_millis(42_789)),
        ..PlaybackState::default()
    };
    let snapshot = Snapshot::project(None, &update(Some(&song), &playback), 0);
    assert_eq!(
        snapshot.track.unwrap().duration,
        Duration::from_millis(42_789)
    );
}

#[test]
fn missing_or_negative_song_duration_is_zero_when_the_stream_has_no_duration() {
    let playback = PlaybackState {
        active: true,
        ..PlaybackState::default()
    };
    for duration in [None, Some(-1), Some(0), Some(42)] {
        let song = Song { duration, ..song() };
        let snapshot = Snapshot::project(None, &update(Some(&song), &playback), 0);
        assert_eq!(
            snapshot.track.unwrap().duration,
            Duration::from_secs(duration.unwrap_or_default().max(0) as u64)
        );
    }
}

#[test]
fn playback_status_and_pause_capability_agree_for_all_transport_states() {
    let song = song();
    for has_song in [false, true] {
        for active in [false, true] {
            for paused in [false, true] {
                for ended in [false, true] {
                    for error in [None, Some("stream failed".into())] {
                        let playback = PlaybackState {
                            active,
                            paused,
                            ended,
                            error: error.clone(),
                            ..PlaybackState::default()
                        };
                        let snapshot = Snapshot::project(
                            None,
                            &update(has_song.then_some(&song), &playback),
                            0,
                        );
                        let expected = match (has_song, ended, error.is_some(), active, paused) {
                            (false, _, _, _, _) | (_, true, _, _, _) | (_, _, true, _, _) => {
                                PlaybackStatus::Stopped
                            }
                            (_, _, _, true, false) => PlaybackStatus::Playing,
                            _ => PlaybackStatus::Paused,
                        };
                        assert_eq!(snapshot.status, expected);
                        assert_eq!(
                            snapshot.can_pause,
                            has_song && active && !ended && error.is_none()
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn transport_progress_reuses_metadata_and_does_not_wake_property_delivery() {
    let song = song();
    let mut playback = PlaybackState {
        active: true,
        ..PlaybackState::default()
    };
    let first = Snapshot::project(None, &update(Some(&song), &playback), 0);
    for seconds in 1..5 {
        playback.position = Duration::from_secs(seconds);
        playback.buffered = Duration::from_secs(seconds + 10);
        playback.loudness_db = seconds as f32;
        let next = Snapshot::project(first.track.as_ref(), &update(Some(&song), &playback), 0);
        assert!(same_track(&first.track, &next.track));
        assert_eq!(next.changes_from(&first), Changes::default());
        assert!(!next.changes_from(&first).any());
        assert_eq!(next.position, Duration::from_secs(seconds));
    }
    assert_eq!(first.position, Duration::ZERO);
}

#[test]
fn revised_metadata_creates_an_immutable_snapshot_without_changing_track_identity() {
    let original = song();
    let playback = PlaybackState::default();
    let first = Snapshot::project(None, &update(Some(&original), &playback), 0);
    for revised in [
        Song {
            title: "New title".into(),
            ..original.clone()
        },
        Song {
            artist: "New artist".into(),
            ..original.clone()
        },
        Song {
            album: "New album".into(),
            ..original.clone()
        },
        Song {
            duration: Some(99),
            ..original.clone()
        },
    ] {
        let next = Snapshot::project(first.track.as_ref(), &update(Some(&revised), &playback), 0);
        assert_eq!(
            next.changes_from(&first),
            Changes {
                metadata: true,
                ..Changes::default()
            }
        );
        assert_eq!(
            next.track.as_ref().unwrap().path,
            first.track.as_ref().unwrap().path
        );
    }
    assert_eq!(first.track.as_ref().unwrap().title, "Title");
    assert_eq!(
        first.track.as_ref().unwrap().duration,
        Duration::from_secs(42)
    );
}

#[test]
fn song_and_server_changes_produce_distinct_track_paths() {
    let original = song();
    let playback = PlaybackState::default();
    let first = Snapshot::project(None, &update(Some(&original), &playback), 0);
    let revised = Song {
        id: "other".into(),
        ..original.clone()
    };
    let other_song = Snapshot::project(first.track.as_ref(), &update(Some(&revised), &playback), 0);
    let mut other_server = update(Some(&original), &playback);
    other_server.server = "https://other.test";
    let other_server = Snapshot::project(first.track.as_ref(), &other_server, 0);
    for next in [other_song, other_server] {
        assert!(next.changes_from(&first).metadata);
        assert_ne!(next.track.unwrap().path, first.track.as_ref().unwrap().path);
    }
}

#[test]
fn late_cover_and_cover_replacement_update_metadata_and_retain_snapshot_handles() {
    let song = song();
    let playback = PlaybackState::default();
    let empty = Snapshot::project(None, &update(Some(&song), &playback), 0);
    let first_cover = cover(1);
    let weak = Arc::downgrade(&first_cover);
    let with_cover = Snapshot::project(
        empty.track.as_ref(),
        &MediaUpdate {
            cover: Some(&first_cover),
            ..update(Some(&song), &playback)
        },
        0,
    );
    assert!(with_cover.changes_from(&empty).metadata);
    drop(first_cover);
    let replacement = cover(2);
    let replaced = Snapshot::project(
        with_cover.track.as_ref(),
        &MediaUpdate {
            cover: Some(&replacement),
            ..update(Some(&song), &playback)
        },
        0,
    );
    assert!(replaced.changes_from(&with_cover).metadata);
    assert!(weak.upgrade().is_some());
    drop(with_cover);
    assert!(weak.upgrade().is_none());
    assert_eq!(
        replaced.track.unwrap().cover.as_ref().unwrap().id,
        replacement.id
    );
}

#[test]
fn separate_cover_handles_with_equal_bytes_reuse_the_same_metadata() {
    let song = song();
    let playback = PlaybackState::default();
    let first_cover = cover(1);
    let same_cover = cover(1);
    assert!(!Arc::ptr_eq(&first_cover, &same_cover));
    assert_eq!(first_cover.id, same_cover.id);
    let first = Snapshot::project(
        None,
        &MediaUpdate {
            cover: Some(&first_cover),
            ..update(Some(&song), &playback)
        },
        0,
    );
    let next = Snapshot::project(
        first.track.as_ref(),
        &MediaUpdate {
            cover: Some(&same_cover),
            ..update(Some(&song), &playback)
        },
        0,
    );
    assert!(same_track(&first.track, &next.track));
    assert!(!next.changes_from(&first).any());
}

#[test]
fn stopping_clears_metadata_but_preserves_queue_play_and_navigation_capabilities() {
    let song = song();
    let playback = PlaybackState {
        active: true,
        ..PlaybackState::default()
    };
    let first = Snapshot::project(None, &update(Some(&song), &playback), 0);
    let stopped = Snapshot::project(
        first.track.as_ref(),
        &update(None, &PlaybackState::default()),
        0,
    );
    assert_eq!(stopped.status, PlaybackStatus::Stopped);
    assert!(stopped.track.is_none());
    assert!(stopped.can_play && stopped.can_next);
    assert!(!stopped.can_pause && !stopped.can_seek);
    assert_eq!(
        stopped.changes_from(&first),
        Changes {
            metadata: true,
            status: true,
            can_pause: true,
            can_seek: true,
            ..Changes::default()
        }
    );
}

#[test]
fn completed_and_restored_seeks_signal_without_republishing_metadata() {
    let song = song();
    let mut playback = PlaybackState::default();
    let first = Snapshot::project(None, &update(Some(&song), &playback), 0);
    playback.position = Duration::from_secs(10);
    for (completed, restored) in [(1, 0), (0, 1), (5, 7)] {
        playback.seek_serial = completed;
        let next = Snapshot::project(
            first.track.as_ref(),
            &update(Some(&song), &playback),
            restored,
        );
        assert_eq!(next.seek_serial, completed + restored);
        assert_eq!(
            next.changes_from(&first),
            Changes {
                seeked: true,
                ..Changes::default()
            }
        );
        assert_eq!(next.position, Duration::from_secs(10));
    }
}

#[test]
fn restored_seek_serial_wraps_without_panicking() {
    let song = song();
    let playback = PlaybackState {
        seek_serial: u64::MAX,
        ..PlaybackState::default()
    };
    let first = Snapshot::project(None, &update(Some(&song), &playback), 0);
    let wrapped = Snapshot::project(first.track.as_ref(), &update(Some(&song), &playback), 1);
    assert_eq!(wrapped.seek_serial, 0);
    assert!(wrapped.changes_from(&first).seeked);
}

#[test]
fn pause_and_error_transitions_signal_transport_changes_without_metadata_changes() {
    let song = song();
    let mut playback = PlaybackState {
        active: true,
        ..PlaybackState::default()
    };
    let first = Snapshot::project(None, &update(Some(&song), &playback), 0);
    playback.paused = true;
    let paused = Snapshot::project(first.track.as_ref(), &update(Some(&song), &playback), 0);
    assert_eq!(
        paused.changes_from(&first),
        Changes {
            status: true,
            ..Changes::default()
        }
    );
    playback.error = Some("failed".into());
    let failed = Snapshot::project(paused.track.as_ref(), &update(Some(&song), &playback), 0);
    assert_eq!(
        failed.changes_from(&paused),
        Changes {
            status: true,
            can_pause: true,
            ..Changes::default()
        }
    );
}

#[test]
fn queue_capabilities_and_volume_changes_signal_only_their_properties() {
    let song = song();
    let playback = PlaybackState::default();
    let first = Snapshot::project(None, &update(Some(&song), &playback), 0);
    let next = Snapshot::project(
        first.track.as_ref(),
        &MediaUpdate {
            can_play: false,
            can_next: false,
            can_previous: true,
            volume: 0.0,
            ..update(Some(&song), &playback)
        },
        0,
    );
    assert_eq!(
        next.changes_from(&first),
        Changes {
            volume: true,
            can_play: true,
            can_seek: true,
            can_next: true,
            can_previous: true,
            ..Changes::default()
        }
    );
    assert_eq!(next.volume, 0.0);
}

#[test]
fn mpris_microseconds_saturate_and_protocol_statuses_remain_english() {
    assert_eq!(micros(Duration::MAX), i64::MAX);
    assert_eq!(micros(Duration::from_nanos(1_234)), 1);
    assert_eq!(micros(Duration::ZERO), 0);
    assert_eq!(PlaybackStatus::Stopped.as_mpris(), "Stopped");
    assert_eq!(PlaybackStatus::Paused.as_mpris(), "Paused");
    assert_eq!(PlaybackStatus::Playing.as_mpris(), "Playing");
}
