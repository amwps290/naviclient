use super::{Command, PlaybackWorker};
use crate::audio::buffered_stream::tests::from_samples;
use crate::audio::decoder::PreparedStream;
use crate::audio::device_output::{AudioOutput, DevicePlayback};
use crate::audio::error::{FailureError, StreamFailure};
use crate::audio::gapless::tests::track;
use crate::audio::gapless::ArmedTrack;
use crate::audio::preload::tests::{client, TestCache};
use crate::audio::preparation_state::{PreparationKind, PreparationToken};
use crate::audio::stream_cache::BufferProgress;
use crate::audio::stream_control::StreamControl;
use crate::audio::track_transition::Transition;
use crate::audio::transport::WorkerCommand;
use crate::audio::PlaybackState;
use crate::models::EqSettings;
use anyhow::anyhow;
use rodio::{ChannelCount, Player, Sample, SampleRate, Source};
use std::fs::File;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn worker(
    cache: &TestCache,
) -> (
    PlaybackWorker,
    Sender<WorkerCommand>,
    impl Source<Item = Sample>,
) {
    let (tx, rx) = mpsc::channel();
    let state = Arc::new(Mutex::new(PlaybackState {
        seek_serial: 7,
        output_name: Some("Headless".into()),
        output_sample_rate: 48_000,
        output_channels: 2,
        ..Default::default()
    }));
    let (player, source) = Player::new();
    let output = AudioOutput::new(DevicePlayback::headless(
        player,
        Some("Headless".into()),
        ChannelCount::new(2).unwrap(),
        SampleRate::new(48_000).unwrap(),
    ));
    (
        PlaybackWorker::new(rx, state, cache.0.clone(), client(), output),
        tx,
        source,
    )
}

fn command(command: Command, serial: u64) -> WorkerCommand {
    WorkerCommand {
        command,
        serial: Some(serial),
    }
}

fn begin(worker: &mut PlaybackWorker, kind: PreparationKind) -> PreparationToken {
    worker.current.begin_play(
        "http://fixture.test/audio".into(),
        "fixture".into(),
        Some(Duration::from_secs(1)),
        1.0,
        Duration::ZERO,
    );
    let position = if kind == PreparationKind::Seek {
        let (position, _) = worker
            .current
            .prepare_seek(Duration::from_millis(25), worker.cache_dir.clone())
            .unwrap();
        position
    } else {
        Duration::ZERO
    };
    worker.preparation.begin(kind, position, Instant::now())
}

fn stream() -> PreparedStream {
    let duration = Some(Duration::from_secs(1));
    let (source, error) = from_samples(
        vec![0.5; 96_000],
        ChannelCount::new(2).unwrap(),
        SampleRate::new(48_000).unwrap(),
        duration,
    );
    PreparedStream {
        source,
        duration,
        error,
        buffer_progress: Arc::new(BufferProgress::new(1, Some(1))),
    }
}

fn assert_diagnostics(state: &PlaybackState, seek_serial: u64) {
    assert_eq!(state.output_name.as_deref(), Some("Headless"));
    assert_eq!(state.output_sample_rate, 48_000);
    assert_eq!(state.output_channels, 2);
    assert_eq!(state.seek_serial, seek_serial);
}

fn arm(worker: &mut PlaybackWorker) -> Arc<StreamControl> {
    let track = track(4800, 48_000, 2, 0.5, 1.0);
    let control = track.info.control.clone();
    *worker.output.handoff.lock().unwrap() = Some(ArmedTrack {
        transition: Transition::new(7, track.info.duration),
        cache_key: "armed".into(),
        track,
    });
    control
}

#[test]
fn transport_commands_update_preparation_intent_and_acknowledge_only_during_periodic_publication() {
    for kind in [PreparationKind::Play, PreparationKind::Seek] {
        let cache = TestCache::new();
        let (mut worker, _tx, _source) = worker(&cache);
        let token = begin(&mut worker, kind);
        for (serial, paused) in [(1, true), (2, false), (3, true)] {
            let action = if paused {
                Command::Pause
            } else {
                Command::Resume
            };
            assert!(worker.handle_command(command(action, serial)).is_none());
            assert_eq!(worker.command_serial, serial);
            assert_eq!(worker.state.lock().unwrap().command_serial, serial - 1);
            assert_eq!(worker.preparation.paused(), paused);
            assert_eq!(worker.output.device.player.is_paused(), paused);
            worker.poll_next_and_publish();
            let state = worker.state.lock().unwrap().clone();
            assert_eq!(state.command_serial, serial);
            assert_eq!(state.paused, paused);
            assert!(state.active);
            assert!(!state.output_connected);
            assert_diagnostics(&state, 7);
        }

        worker.handle_command(command(Command::Stop, 4));
        assert!(!worker.preparation.preparing());
        assert!(!worker.preparation.complete(token));
        assert_eq!(worker.current.duration(), None);
        worker.poll_next_and_publish();
        let state = worker.state.lock().unwrap().clone();
        assert_eq!(state.command_serial, 4);
        assert!(!state.active);
        assert!(!state.ended);
        assert!(!state.output_connected);
        assert!(state.error.is_none());
        assert_diagnostics(&state, 7);
    }
}

#[test]
fn preference_commands_keep_transport_acknowledgements_and_share_the_existing_equalizer_handle() {
    let cache = TestCache::new();
    let (mut worker, _tx, _source) = worker(&cache);
    worker.handle_command(command(Command::Pause, 5));
    let eq = worker.output.eq.clone();
    for volume in [-2.0_f32, 0.0, 0.25, 1.0, 3.0] {
        worker.handle_command(Command::SetVolume(volume).into());
        assert_eq!(worker.volume, volume.clamp(0.0, 1.0));
        assert_eq!(worker.output.device.player.volume(), worker.volume);
    }
    eq.set_version_for_test(u64::MAX);
    let settings = EqSettings {
        enabled: true,
        gains_db: [3.0; 10],
    };
    worker.handle_command(Command::SetEqualizer(settings.clone()).into());
    assert!(worker.output.eq.shares_state(&eq));
    assert_eq!(eq.snapshot(), (0, settings));
    worker.poll_next_and_publish();
    let state = worker.state.lock().unwrap().clone();
    assert_eq!(state.command_serial, 5);
    assert!(state.paused);
    assert_diagnostics(&state, 7);
}

#[test]
fn coalesced_seeks_keep_the_last_serial_and_preserve_the_intervening_command_queue() {
    let cache = TestCache::new();
    let (mut worker, tx, _source) = worker(&cache);
    tx.send(command(Command::Seek(Duration::from_secs(20)), 2))
        .unwrap();
    tx.send(command(Command::Seek(Duration::from_secs(30)), 3))
        .unwrap();
    tx.send(command(Command::Pause, 4)).unwrap();
    tx.send(command(Command::Resume, 5)).unwrap();
    // No source is selected, so this exercises command order without HTTP I/O.
    assert!(worker
        .handle_command(command(Command::Seek(Duration::from_secs(10)), 1))
        .is_none());
    assert_eq!(worker.command_serial, 3);
    assert!(!worker.preparation.preparing());
    assert_eq!(worker.pending_commands.len(), 1);
    let pending = worker.pending_commands.pop_front().unwrap();
    assert!(matches!(pending.command, Command::Pause));
    worker.handle_command(pending);
    assert_eq!(worker.command_serial, 4);
    let next = worker.rx.try_recv().unwrap();
    assert!(matches!(next.command, Command::Resume));
    worker.handle_command(next);
    worker.handle_command(Command::SetVolume(0.5).into());
    worker.poll_next_and_publish();
    assert_eq!(worker.state.lock().unwrap().command_serial, 5);
    assert!(worker.pending_commands.is_empty());
    assert!(matches!(
        worker.rx.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
}

#[test]
fn prepared_output_uses_the_latest_pause_and_volume_and_increments_only_seek_completion() {
    for kind in [PreparationKind::Play, PreparationKind::Seek] {
        for paused in [false, true] {
            let cache = TestCache::new();
            let (mut worker, _tx, mut source) = worker(&cache);
            let token = begin(&mut worker, kind);
            worker.preparation.deliver(token, Ok(stream()));
            worker.handle_command(Command::SetVolume(0.25).into());
            worker.handle_command(command(
                if paused {
                    Command::Pause
                } else {
                    Command::Resume
                },
                1,
            ));
            let mut ready = None;
            worker.poll_preparation(&mut ready);
            assert!(!worker.preparation.preparing());
            assert!(worker.output.device.player.empty());
            worker.attach_output(ready.unwrap());
            assert_eq!(worker.output.device.player.is_paused(), paused);
            assert_eq!(worker.output.device.player.volume(), 0.25);
            worker.poll_next_and_publish();
            let state = worker.state.lock().unwrap().clone();
            assert!(state.active);
            assert!(state.output_connected);
            assert_eq!(state.paused, paused);
            assert_eq!(state.command_serial, 1);
            assert_eq!(state.position, token.position);
            assert_diagnostics(&state, 7 + u64::from(kind == PreparationKind::Seek));
            if !paused {
                assert_eq!(source.next(), Some(0.125));
            }
            let mut duplicate = None;
            worker.poll_preparation(&mut duplicate);
            assert!(duplicate.is_none());
            assert_eq!(worker.state.lock().unwrap().seek_serial, state.seek_serial);
        }
    }
}

#[test]
fn preparation_failures_and_cancelled_successes_cannot_attach_output_or_clear_a_newer_request() {
    for kind in [PreparationKind::Play, PreparationKind::Seek] {
        let cache = TestCache::new();
        let (mut worker, _tx, _source) = worker(&cache);
        let old = begin(&mut worker, kind);
        let current = begin(&mut worker, kind);
        let stale = stream();
        let stale_control = stale.source.control().clone();
        worker.preparation.deliver(old, Ok(stale));
        let mut ready = None;
        worker.poll_preparation(&mut ready);
        assert!(ready.is_none());
        assert!(worker.preparation.preparing());
        assert!(stale_control.check().is_err());

        worker.preparation.deliver(
            current,
            Err(anyhow!(FailureError::new(
                StreamFailure::Decode,
                "injected failure"
            ))),
        );
        worker.poll_preparation(&mut ready);
        assert!(ready.is_none());
        assert!(!worker.preparation.preparing());
        let state = worker.state.lock().unwrap().clone();
        assert_eq!(state.failure, Some(StreamFailure::Decode));
        assert!(state.error.as_deref().unwrap().contains("injected failure"));
        assert!(!state.active);
        assert!(!state.ended);
        assert_diagnostics(&state, 7);

        let pending = begin(&mut worker, kind);
        let success = stream();
        let success_control = success.source.control().clone();
        worker.preparation.deliver(pending, Ok(success));
        worker.handle_command(command(Command::Stop, 1));
        worker.poll_preparation(&mut ready);
        assert!(ready.is_none());
        assert!(success_control.check().is_err());
        assert!(worker.output.device.player.empty());
    }
}

#[test]
fn cache_directory_success_and_failure_both_cancel_handoffs_but_only_success_replaces_the_directory(
) {
    let cache = TestCache::new();
    let (mut worker, _tx, _source) = worker(&cache);
    let old = worker.cache_dir.clone();
    let blocked = old.join("blocked");
    File::create(&blocked).unwrap();
    let control = arm(&mut worker);
    worker.handle_command(Command::SetCacheDirectory(blocked).into());
    assert_eq!(worker.cache_dir, old);
    assert!(worker.output.handoff.lock().unwrap().is_none());
    assert!(control.check().is_err());

    let new = old.join("new-cache");
    let control = arm(&mut worker);
    worker.handle_command(Command::SetCacheDirectory(new.clone()).into());
    assert_eq!(worker.cache_dir, new);
    assert!(new.is_dir());
    assert!(worker.output.handoff.lock().unwrap().is_none());
    assert!(control.check().is_err());
}

#[test]
fn shutdown_drains_already_pending_commands_before_a_disconnected_input_exits() {
    let cache = TestCache::new();
    let (mut worker, tx, _source) = worker(&cache);
    let state = worker.state.clone();
    worker
        .pending_commands
        .push_back(command(Command::Pause, 4));
    worker
        .pending_commands
        .push_back(Command::SetVolume(0.25).into());
    worker.pending_commands.push_back(command(Command::Stop, 5));
    drop(tx);
    worker.run();
    let state = state.lock().unwrap().clone();
    assert_eq!(state.command_serial, 5);
    assert!(!state.active);
    assert!(!state.ended);
    assert_diagnostics(&state, 7);
}

#[test]
fn dropping_the_worker_releases_undelivered_preparation_and_the_published_handoff() {
    let cache = TestCache::new();
    let (mut worker, _tx, _source) = worker(&cache);
    let token = begin(&mut worker, PreparationKind::Play);
    let prepared = stream();
    let foreground_control = prepared.source.control().clone();
    worker.preparation.deliver(token, Ok(prepared));
    let handoff_control = arm(&mut worker);
    let handoff = worker.output.handoff.clone();

    drop(worker);

    assert!(foreground_control.check().is_err());
    assert!(handoff_control.check().is_err());
    assert!(handoff.lock().unwrap().is_none());
}
