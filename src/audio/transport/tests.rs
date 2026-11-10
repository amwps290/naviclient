use super::*;
use crate::audio::{AudioHandle, StreamFailure};
use std::sync::mpsc::Receiver;
use std::thread;
use std::time::Duration;

fn handle(playback: PlaybackState) -> (AudioHandle, Receiver<WorkerCommand>) {
    let (tx, rx) = mpsc::channel();
    (
        AudioHandle {
            tx,
            state: Arc::new(Mutex::new(playback)),
            transition: Arc::new(Mutex::new(None)),
            transport: Arc::new(Mutex::new(Transport::default())),
        },
        rx,
    )
}

fn playing() -> PlaybackState {
    PlaybackState {
        active: true,
        output_connected: true,
        position: Duration::from_secs(20),
        duration: Some(Duration::from_secs(120)),
        ..Default::default()
    }
}

#[test]
fn older_seek_acknowledgement_cannot_clear_a_newer_pending_target() {
    let (audio, rx) = handle(playing());
    audio.seek(Duration::from_secs(25));
    let first = rx.recv().unwrap().serial.unwrap();
    audio.seek(Duration::from_secs(30));
    let last = rx.recv().unwrap().serial.unwrap();
    assert_ne!(first, last);
    *audio.state.lock().unwrap() = PlaybackState {
        command_serial: first,
        position: Duration::from_secs(25),
        ..playing()
    };
    assert_eq!(audio.state().position, Duration::from_secs(30));
    *audio.state.lock().unwrap() = PlaybackState {
        command_serial: last,
        position: Duration::from_secs(31),
        ..playing()
    };
    assert_eq!(audio.state().position, Duration::from_secs(31));
    audio.state.lock().unwrap().position = Duration::from_secs(32);
    assert_eq!(audio.state().position, Duration::from_secs(32));
}

#[test]
fn selection_discards_previous_track_diagnostics_and_preserves_device_information() {
    let (audio, rx) = handle(PlaybackState {
        ended: true,
        error: Some("previous stream failed".into()),
        failure: Some(StreamFailure::Timeout),
        buffered: Duration::from_secs(100),
        fully_buffered: true,
        transition_id: Some(7),
        stall: Some(Duration::from_secs(10)),
        loudness_db: -8.0,
        loudness_peak: 0.75,
        seek_serial: 3,
        output_name: Some("device".into()),
        output_channels: 2,
        output_sample_rate: 48_000,
        ..playing()
    });
    audio.play(
        "test".into(),
        "next".into(),
        Some(Duration::from_secs(10)),
        1.0,
        crate::audio::PlaybackStart {
            position: Duration::from_secs(20),
            ..Default::default()
        },
    );
    let serial = rx.recv().unwrap().serial.unwrap();
    let projected = audio.state();
    assert_eq!(projected.position, Duration::from_secs(10));
    assert_eq!(projected.duration, Some(Duration::from_secs(10)));
    assert!(projected.active && !projected.paused);
    assert!(!projected.ended && !projected.fully_buffered);
    assert_eq!(projected.error, None);
    assert_eq!(projected.failure, None);
    assert_eq!(projected.stall, None);
    assert_eq!(projected.transition_id, None);
    assert_eq!(projected.buffered, Duration::ZERO);
    assert_eq!(projected.loudness_db, 0.0);
    assert_eq!(projected.loudness_peak, 0.0);
    assert_eq!(projected.seek_serial, 3);
    assert_eq!(projected.output_name.as_deref(), Some("device"));
    assert_eq!(projected.output_channels, 2);
    assert_eq!(projected.output_sample_rate, 48_000);
    audio.state.lock().unwrap().command_serial = serial;
    audio.state.lock().unwrap().error = Some("current stream failed".into());
    assert_eq!(
        audio.state().error.as_deref(),
        Some("current stream failed")
    );
}

#[test]
fn paused_start_is_one_command_and_a_later_resume_wins() {
    let (audio, rx) = handle(playing());
    assert!(audio.play(
        "test".into(),
        "next".into(),
        Some(Duration::from_secs(10)),
        1.0,
        crate::audio::PlaybackStart {
            position: Duration::from_secs(5),
            paused: true,
            ..Default::default()
        },
    ));
    let projected = audio.state();
    assert!(projected.active && projected.paused);
    assert_eq!(projected.position, Duration::from_secs(5));
    assert!(matches!(
        rx.recv().unwrap().command,
        Command::Play { start, .. } if start.paused
    ));
    assert!(rx.try_recv().is_err());
    audio.resume();
    assert!(!audio.state().paused);
    assert!(matches!(rx.recv().unwrap().command, Command::Resume));
}

#[test]
fn commands_after_selection_use_selected_bounds_and_latest_pause_intent() {
    let (audio, rx) = handle(playing());
    audio.play(
        "test".into(),
        "next".into(),
        None,
        1.0,
        crate::audio::PlaybackStart {
            position: Duration::from_secs(50),
            ..Default::default()
        },
    );
    audio.pause();
    audio.seek(Duration::from_secs(200));
    let projected = audio.state();
    assert_eq!(projected.duration, None);
    assert_eq!(projected.position, Duration::from_secs(200));
    assert!(projected.active && projected.paused);
    let commands: Vec<_> = rx.try_iter().collect();
    assert_eq!(commands.len(), 3);
    assert!(commands
        .windows(2)
        .all(|pair| pair[0].serial < pair[1].serial));
    audio.resume();
    assert!(!audio.state().paused);
    audio.pause();
    assert!(audio.state().paused);
}

#[test]
fn stop_is_visible_before_worker_poll_and_old_success_cannot_restore_playing_state() {
    let (audio, rx) = handle(playing());
    audio.seek(Duration::from_secs(25));
    let old = rx.recv().unwrap().serial.unwrap();
    audio.stop();
    let stop = rx.recv().unwrap().serial.unwrap();
    audio.state.lock().unwrap().command_serial = old;
    let projected = audio.state();
    assert!(!projected.active && !projected.ended);
    assert_eq!(projected.position, Duration::ZERO);
    assert_eq!(projected.duration, None);
    *audio.state.lock().unwrap() = PlaybackState {
        command_serial: stop,
        ..Default::default()
    };
    assert!(!audio.state().active);
}

#[test]
fn untracked_commands_do_not_discard_pending_seek_and_diagnostics_keep_updating() {
    let (audio, rx) = handle(playing());
    audio.seek(Duration::from_secs(30));
    let seek = rx.recv().unwrap().serial.unwrap();
    audio.set_volume(0.5);
    assert_eq!(rx.recv().unwrap().serial, None);
    audio.state.lock().unwrap().output_name = Some("new device".into());
    audio.state.lock().unwrap().seek_serial = 4;
    let projected = audio.state();
    assert_eq!(projected.position, Duration::from_secs(30));
    assert_eq!(projected.output_name.as_deref(), Some("new device"));
    assert_eq!(projected.seek_serial, 4);
    assert_ne!(projected.command_serial, seek);
}

#[test]
fn failed_delivery_does_not_leave_an_invented_preparing_state() {
    let (audio, rx) = handle(PlaybackState::default());
    drop(rx);
    let accepted = audio.play("test".into(), "next".into(), None, 1.0, Default::default());
    assert!(!accepted);
    assert!(!audio.state().active);
}

#[test]
fn worker_exit_exposes_initialization_failure_instead_of_retaining_pending_play() {
    let (audio, _rx) = handle(PlaybackState::default());
    audio.play("test".into(), "next".into(), None, 1.0, Default::default());
    assert!(audio.state().active);
    audio.state.lock().unwrap().error = Some("output unavailable".into());
    drop(WorkerGuard(audio.state.clone()));
    let observed = audio.state();
    assert!(!observed.active && !observed.ended);
    assert!(observed.worker_closed);
    assert!(!audio.progress().active);
    assert_eq!(observed.error.as_deref(), Some("output unavailable"));
}

#[test]
fn progress_waits_for_replacement_and_its_following_commands_then_for_output() {
    let (audio, rx) = handle(playing());
    audio.play(
        "test".into(),
        "next".into(),
        None,
        1.0,
        crate::audio::PlaybackStart {
            position: Duration::from_secs(5),
            ..Default::default()
        },
    );
    let play = rx.recv().unwrap().serial.unwrap();
    audio.pause();
    let pause = rx.recv().unwrap().serial.unwrap();
    assert!(audio.state().active);
    assert!(!audio.progress().active);
    assert_eq!(audio.progress().position, Duration::from_secs(5));
    // Neither an old ready track nor an intermediate publication may make the
    // pending replacement look ready, including publications between commands.
    *audio.state.lock().unwrap() = PlaybackState {
        command_serial: play,
        ..playing()
    };
    assert!(!audio.progress().active);
    *audio.state.lock().unwrap() = PlaybackState {
        command_serial: pause,
        output_connected: false,
        paused: true,
        position: Duration::from_secs(5),
        ..playing()
    };
    assert!(audio.state().active);
    assert!(!audio.progress().active);
    audio.state.lock().unwrap().output_connected = true;
    assert!(audio.progress().active && audio.progress().paused);
    audio.stop();
    assert!(!audio.progress().active);
    assert_eq!(audio.progress().position, Duration::ZERO);
}

#[test]
fn progress_uses_observed_position_for_pause_and_waits_for_actual_resume() {
    let (audio, rx) = handle(playing());
    audio.pause();
    let pause = rx.recv().unwrap().serial.unwrap();
    audio.state.lock().unwrap().position = Duration::from_secs(21);
    assert_eq!(audio.state().position, Duration::from_secs(20));
    let progress = audio.progress();
    assert_eq!(progress.position, Duration::from_secs(21));
    assert!(progress.active && progress.paused);
    *audio.state.lock().unwrap() = PlaybackState {
        command_serial: pause,
        paused: true,
        ..playing()
    };
    audio.resume();
    let resume = rx.recv().unwrap().serial.unwrap();
    assert!(!audio.state().paused);
    assert!(audio.progress().paused);
    *audio.state.lock().unwrap() = PlaybackState {
        command_serial: resume,
        ..playing()
    };
    assert!(audio.progress().active && !audio.progress().paused);
}

#[test]
fn pending_seeks_supply_inactive_baselines_and_only_completed_output_is_active() {
    let (audio, rx) = handle(playing());
    audio.seek(Duration::from_secs(21));
    let seek = rx.recv().unwrap().serial.unwrap();
    assert!(!audio.progress().active);
    assert_eq!(audio.progress().position, Duration::from_secs(21));
    *audio.state.lock().unwrap() = PlaybackState {
        command_serial: seek,
        output_connected: false,
        position: Duration::from_secs(21),
        ..playing()
    };
    assert!(audio.state().active);
    assert!(!audio.progress().active);
    audio.state.lock().unwrap().output_connected = true;
    assert!(audio.progress().active);
    audio.state.lock().unwrap().position = Duration::from_millis(21_250);
    assert_eq!(audio.progress().position, Duration::from_millis(21_250));
}

#[test]
fn preparation_failure_publishes_no_active_progress_and_retains_its_error() {
    let (audio, rx) = handle(playing());
    audio.play("test".into(), "bad".into(), None, 1.0, Default::default());
    let serial = rx.recv().unwrap().serial.unwrap();
    *audio.state.lock().unwrap() = PlaybackState {
        command_serial: serial,
        error: Some("preparation failed".into()),
        failure: Some(StreamFailure::Decode),
        ..Default::default()
    };
    let progress = audio.progress();
    assert!(!progress.active);
    assert_eq!(progress.error.as_deref(), Some("preparation failed"));
    assert_eq!(progress.failure, Some(StreamFailure::Decode));
}

#[test]
fn sequence_wrap_does_not_confuse_default_worker_state_with_an_acknowledgement() {
    let (audio, rx) = handle(PlaybackState::default());
    audio.transport.lock().unwrap().serial = u64::MAX;
    audio.play("test".into(), "next".into(), None, 1.0, Default::default());
    assert_eq!(rx.recv().unwrap().serial, Some(1));
    assert!(audio.state().active);
}

#[test]
fn cloned_handles_share_one_command_order_and_the_latest_pending_position() {
    let (audio, rx) = handle(playing());
    let barrier = Arc::new(std::sync::Barrier::new(4));
    let workers: Vec<_> = (0..4)
        .map(|worker| {
            let audio = audio.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                for offset in 0..8 {
                    audio.seek(Duration::from_secs(worker * 8 + offset));
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    let commands: Vec<_> = rx.try_iter().collect();
    assert_eq!(commands.len(), 32);
    for (index, command) in commands.iter().enumerate() {
        assert_eq!(command.serial, Some(index as u64 + 1));
    }
    let Command::Seek(last_position) = commands.last().unwrap().command else {
        panic!("expected seek command");
    };
    assert_eq!(audio.state().position, last_position);
}
