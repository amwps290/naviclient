use std::collections::VecDeque;
use std::io::{self, SeekFrom};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::anyhow;
use rodio::source::UniformSourceIterator;
use rodio::{ChannelCount, Player, SampleRate, Source};

use super::decoder::stream_ended_too_early;
use super::error::{classify_anyhow, classify_io, failure_io, FailureError};
use super::http_stream::{logical_playback_position, near_estimated_end, seek_target};
use super::loudness_meter::LoudnessMeter;
use super::output::normalize_for_output;
use super::stream_control::StreamControl;
use super::transport::{Transport, WorkerCommand};
use super::worker::commands::coalesce_seek_commands;
use super::worker::playback::apply_playback_intent;
use super::worker::Command;
use super::{test_transport, AudioHandle, PlaybackState, StreamFailure};

#[test]
fn preload_acceptance_preserves_current_playback_and_delivers_the_reservation() {
    let (tx, rx) = mpsc::channel();
    let playback = PlaybackState {
        active: true,
        output_connected: true,
        position: Duration::from_secs(165),
        duration: Some(Duration::from_secs(180)),
        fully_buffered: true,
        ..Default::default()
    };
    let audio = AudioHandle {
        tx,
        state: Arc::new(Mutex::new(playback.clone())),
        transition: Arc::new(Mutex::new(None)),
        transport: Arc::new(Mutex::new(Transport::default())),
    };
    assert!(audio.preload(
        42,
        "fixture".into(),
        "next".into(),
        Some(Duration::from_secs(120)),
        0.75
    ));
    let message = rx.try_recv().unwrap();
    assert!(message.serial.is_none());
    let Command::Preload {
        id,
        url,
        cache_key,
        duration_hint,
        gain,
        transition,
    } = message.command
    else {
        panic!("expected one preload command");
    };
    assert_eq!(id, 42);
    assert_eq!(url.url(), "fixture");
    assert_eq!(cache_key, "next");
    assert_eq!(duration_hint, Some(Duration::from_secs(120)));
    assert_eq!(gain, 0.75);
    assert!(transition.pending());
    assert!(Arc::ptr_eq(
        &transition,
        audio.transition.lock().unwrap().as_ref().unwrap()
    ));
    assert_eq!(audio.state(), playback);
    assert_eq!(audio.progress(), playback);
    assert!(rx.try_recv().is_err());
}

#[test]
fn rejected_preload_cancels_its_unsubmitted_transition() {
    let mut fixture = test_transport::Fixture::new(PlaybackState::default());
    fixture.disconnect();
    assert!(!fixture
        .audio
        .preload(42, "fixture".into(), "song".into(), None, 1.0));
    let transition = fixture.audio.transition.lock().unwrap().clone().unwrap();
    assert!(!transition.pending());
    assert!(transition.event().is_none());
    assert!(fixture.audio.freeze_preload().is_none());
    assert!(!fixture.audio.progress().active);
}

#[test]
fn network_failures_are_retryable_but_decode_and_output_are_not() {
    for kind in [
        StreamFailure::Connect,
        StreamFailure::Timeout,
        StreamFailure::NoData,
        StreamFailure::Truncated,
        StreamFailure::Http(503),
        StreamFailure::Http(429),
    ] {
        assert!(kind.is_retryable(), "{kind:?} should be retried");
    }
    for kind in [
        StreamFailure::Decode,
        StreamFailure::Output,
        StreamFailure::Preparation,
        StreamFailure::Http(404),
    ] {
        assert!(!kind.is_retryable(), "{kind:?} should not be retried");
    }
}

#[test]
fn failure_messages_keep_the_summary_and_the_detail() {
    let failure = FailureError::new(StreamFailure::Http(502), "audio request returned HTTP 502");
    assert_eq!(
        failure.message(),
        "Server returned an error: audio request returned HTTP 502"
    );
    // 归因提示要能把网络与服务端区分开，而不是只说「失败」。
    assert!(StreamFailure::Connect.cause().starts_with("Networking"));
    assert!(StreamFailure::Http(500).cause().starts_with("Server"));
    assert!(StreamFailure::Output.cause().starts_with("Local"));
}

#[test]
fn io_errors_carry_their_classification_through_the_reader_chain() {
    let wrapped = failure_io(StreamFailure::Truncated, "stream ended at byte 10 of 90");
    assert_eq!(classify_io(&wrapped), StreamFailure::Truncated);
    assert_eq!(
        classify_io(&io::Error::from(io::ErrorKind::TimedOut)),
        StreamFailure::Timeout
    );
    assert_eq!(
        classify_io(&io::Error::from(io::ErrorKind::ConnectionRefused)),
        StreamFailure::Connect
    );
    assert_eq!(
        classify_io(&io::Error::from(io::ErrorKind::UnexpectedEof)),
        StreamFailure::Truncated
    );
}

#[test]
fn anyhow_wrapping_preserves_the_failure_kind() {
    let anyhow_error = anyhow!(FailureError::new(StreamFailure::NoData, "no audio"));
    assert_eq!(classify_anyhow(&anyhow_error), StreamFailure::NoData);
    let contexted = anyhow_error.context("failed to prepare stream");
    assert_eq!(classify_anyhow(&contexted), StreamFailure::NoData);
}

#[test]
fn restoring_playback_sends_one_command_with_the_saved_position() {
    let (tx, rx) = mpsc::channel();
    let audio = AudioHandle {
        tx,
        state: Arc::new(Mutex::new(PlaybackState::default())),
        transition: Arc::new(Mutex::new(None)),
        transport: Arc::new(Mutex::new(Transport::default())),
    };
    audio.play(
        "test".into(),
        "song".into(),
        Some(Duration::from_secs(180)),
        1.0,
        crate::audio::PlaybackStart {
            position: Duration::from_secs(42),
            ..Default::default()
        },
    );
    assert!(
        matches!(rx.recv().unwrap().command, Command::Play { start, .. } if start.position == Duration::from_secs(42))
    );
    assert!(rx.try_recv().is_err());
}

#[test]
fn relative_seeks_accumulate_before_the_worker_processes_the_commands() {
    let (tx, _rx) = mpsc::channel();
    let audio = AudioHandle {
        tx,
        state: Arc::new(Mutex::new(PlaybackState {
            active: true,
            position: Duration::from_secs(20),
            duration: Some(Duration::from_secs(120)),
            ..Default::default()
        })),
        transition: Arc::new(Mutex::new(None)),
        transport: Arc::new(Mutex::new(Transport::default())),
    };
    let song = crate::models::Song {
        duration: Some(120),
        ..Default::default()
    };
    for _ in 0..2 {
        let timeline = crate::playback_timeline::PlaybackTimeline::for_song(&audio.state(), &song);
        audio.seek(timeline.seek_seconds(5));
    }
    assert_eq!(audio.state().position, Duration::from_secs(30));
}

#[test]
fn newly_selected_track_supplies_its_own_position_and_duration_before_worker_poll() {
    let (tx, _rx) = mpsc::channel();
    let audio = AudioHandle {
        tx,
        state: Arc::new(Mutex::new(PlaybackState {
            active: true,
            position: Duration::from_secs(100),
            duration: Some(Duration::from_secs(120)),
            ended: true,
            ..Default::default()
        })),
        transition: Arc::new(Mutex::new(None)),
        transport: Arc::new(Mutex::new(Transport::default())),
    };
    audio.play(
        "test".into(),
        "new-song".into(),
        Some(Duration::from_secs(180)),
        1.0,
        Default::default(),
    );
    let song = crate::models::Song {
        duration: Some(180),
        ..Default::default()
    };
    let timeline = crate::playback_timeline::PlaybackTimeline::for_song(&audio.state(), &song);
    assert_eq!(timeline.position, Duration::ZERO);
    assert_eq!(timeline.duration, Some(Duration::from_secs(180)));
    assert!(!audio.state().ended);
    audio.seek(timeline.seek_seconds(5));
    assert_eq!(audio.state().position, Duration::from_secs(5));
}

#[test]
fn prepared_audio_obeys_current_intent_instead_of_the_cleared_sink() {
    let (player, _source) = Player::new();
    player.stop();
    player.clear();
    assert!(player.is_paused());
    apply_playback_intent(&player, false);
    assert!(!player.is_paused());
    // Pause/resume commands arriving during preparation must win when ready.
    apply_playback_intent(&player, true);
    assert!(player.is_paused());
    apply_playback_intent(&player, false);
    assert!(!player.is_paused());
}

#[test]
fn coalesces_slider_seek_events_to_the_latest_position() {
    let (sender, receiver) = mpsc::channel::<WorkerCommand>();
    sender
        .send(WorkerCommand {
            command: Command::Seek(Duration::from_secs(20)),
            serial: Some(1),
        })
        .unwrap();
    sender
        .send(WorkerCommand {
            command: Command::Seek(Duration::from_secs(30)),
            serial: Some(2),
        })
        .unwrap();
    sender
        .send(WorkerCommand {
            command: Command::Pause,
            serial: Some(3),
        })
        .unwrap();

    let first = receiver.recv().unwrap();
    let Command::Seek(initial) = first.command else {
        panic!("expected seek command");
    };
    let mut pending = VecDeque::new();
    let (latest, serial, count) =
        coalesce_seek_commands(initial, first.serial, &receiver, &mut pending);

    assert_eq!(latest, Duration::from_secs(30));
    assert_eq!(serial, Some(2));
    assert_eq!(count, 2);
    assert!(matches!(
        pending.pop_front(),
        Some(WorkerCommand {
            command: Command::Pause,
            serial: Some(3),
        })
    ));
}

#[test]
fn seek_coalescing_preserves_intervening_transport_commands() {
    let (tx, rx) = mpsc::channel();
    tx.send(Command::Pause.into()).unwrap();
    tx.send(Command::Seek(Duration::from_secs(30)).into())
        .unwrap();
    let mut pending = VecDeque::new();
    let (position, _, count) =
        coalesce_seek_commands(Duration::from_secs(10), None, &rx, &mut pending);
    assert_eq!((position, count), (Duration::from_secs(10), 1));
    assert!(matches!(
        pending.pop_front(),
        Some(WorkerCommand {
            command: Command::Pause,
            ..
        })
    ));
    assert!(
        matches!(rx.recv().unwrap().command, Command::Seek(position) if position == Duration::from_secs(30))
    );
}

#[test]
fn loudness_meter_reports_rms_and_peak() {
    struct Constant {
        value: f32,
        remaining: usize,
        sample_rate: SampleRate,
    }
    impl Iterator for Constant {
        type Item = f32;
        fn next(&mut self) -> Option<Self::Item> {
            if self.remaining == 0 {
                None
            } else {
                self.remaining -= 1;
                Some(self.value)
            }
        }
    }
    impl Source for Constant {
        fn current_span_len(&self) -> Option<usize> {
            Some(self.remaining)
        }
        fn channels(&self) -> ChannelCount {
            ChannelCount::new(1).unwrap()
        }
        fn sample_rate(&self) -> SampleRate {
            self.sample_rate
        }
        fn total_duration(&self) -> Option<Duration> {
            None
        }
    }

    let shared = Arc::new(Mutex::new(None::<(f32, f32)>));
    // 振幅 0.5 的恒信号：RMS = 0.5 → ≈ -6.02 dB，峰值 = 0.5。
    let meter = LoudnessMeter::new(
        Constant {
            value: 0.5,
            remaining: 20_000,
            sample_rate: SampleRate::new(44_100).unwrap(),
        },
        shared.clone(),
    );
    // 排空迭代器，触发共享读数更新。
    let _: Vec<f32> = meter.collect();
    let (rms_db, peak) = shared.lock().unwrap().expect("meter should have reported");
    assert!((rms_db - 20.0 * 0.5f32.log10()).abs() < 0.1);
    assert!((peak - 0.5).abs() < 0.001);
}

#[test]
fn computes_stream_seek_targets() {
    assert_eq!(seek_target(20, Some(100), SeekFrom::Start(5)).unwrap(), 5);
    assert_eq!(
        seek_target(20, Some(100), SeekFrom::Current(-5)).unwrap(),
        15
    );
    assert_eq!(seek_target(20, Some(100), SeekFrom::End(-5)).unwrap(), 95);
}

#[test]
fn rejects_invalid_stream_seek_targets() {
    assert!(seek_target(0, Some(100), SeekFrom::Current(-1)).is_err());
    assert!(seek_target(0, Some(100), SeekFrom::Start(101)).is_err());
    assert!(seek_target(0, None, SeekFrom::End(0)).is_err());
}

#[test]
fn distinguishes_truncated_streams_from_natural_ends() {
    let channels = ChannelCount::new(2).unwrap();
    let sample_rate = SampleRate::new(48_000).unwrap();
    let samples_per_second = u64::from(channels.get()) * u64::from(sample_rate.get());

    assert!(stream_ended_too_early(
        samples_per_second * 20,
        channels,
        sample_rate,
        Some(Duration::from_secs(180)),
    ));
    assert!(!stream_ended_too_early(
        samples_per_second * 178,
        channels,
        sample_rate,
        Some(Duration::from_secs(180)),
    ));
}

#[test]
fn synthetic_stall_silence_does_not_advance_the_track_position() {
    let control = StreamControl::new(false);
    let sample_rate = SampleRate::new(1_000).unwrap();
    for _ in 0..2_500 {
        control.mark_synthetic_silence_frame(sample_rate);
    }

    let synthetic_silence = control.synthetic_silence_duration();
    assert_eq!(synthetic_silence, Duration::from_millis(2_500));
    assert_eq!(
        logical_playback_position(
            Duration::from_secs(40),
            Duration::from_millis(12_500),
            synthetic_silence,
        ),
        Duration::from_secs(50)
    );
    assert_eq!(
        logical_playback_position(
            Duration::from_secs(40),
            Duration::from_secs(1),
            Duration::from_secs(2),
        ),
        Duration::from_secs(40)
    );
}

#[test]
fn accepts_small_transcode_length_estimation_errors_only_near_the_end() {
    assert!(near_estimated_end(9_399_881, 9_623_552));
    assert!(near_estimated_end(95, 100));
    assert!(!near_estimated_end(94, 100));
    assert!(!near_estimated_end(0, 0));
}

#[test]
fn normalizes_each_song_before_it_enters_the_shared_player_queue() {
    use rodio::Player;

    // BufferedStreamSource 在播放期间上报无限 span。若把不同采样率的源直接放进
    // 长期复用的 Player 队列，rodio 会让后续歌曲沿用第一首的采样率。
    struct FakeSong {
        samples: std::vec::IntoIter<f32>,
        channels: ChannelCount,
        sample_rate: SampleRate,
    }

    impl Iterator for FakeSong {
        type Item = f32;

        fn next(&mut self) -> Option<Self::Item> {
            self.samples.next()
        }
    }

    impl Source for FakeSong {
        fn current_span_len(&self) -> Option<usize> {
            None
        }

        fn channels(&self) -> ChannelCount {
            self.channels
        }

        fn sample_rate(&self) -> SampleRate {
            self.sample_rate
        }

        fn total_duration(&self) -> Option<Duration> {
            None
        }
    }

    let channels = ChannelCount::new(2).unwrap();
    let output_rate = SampleRate::new(48_000).unwrap();
    let (player, queue) = Player::new();
    player.append(normalize_for_output(
        FakeSong {
            samples: vec![0.5_f32; 88_200].into_iter(),
            channels,
            sample_rate: SampleRate::new(44_100).unwrap(),
        },
        channels,
        output_rate,
    ));
    player.append(normalize_for_output(
        FakeSong {
            samples: vec![0.25_f32; 192_000].into_iter(),
            channels,
            sample_rate: SampleRate::new(96_000).unwrap(),
        },
        channels,
        output_rate,
    ));

    // 模拟设备 mixer 对共享 Player 队列再做一次统一格式检查。
    let mut output = UniformSourceIterator::new(queue, channels, output_rate);
    let mut first_count = 0usize;
    let mut second_count = 0usize;
    let mut silence_run = 0usize;
    for sample in output.by_ref() {
        if sample == 0.5 {
            first_count += 1;
            silence_run = 0;
        } else if sample == 0.25 {
            second_count += 1;
            silence_run = 0;
        } else {
            silence_run += 1;
            if silence_run > 100_000 {
                break;
            }
        }
    }

    // 两首都是 1 秒立体声，规范到 48kHz 后均应约为 96_000 个样本。
    assert!(
        (96_000.0_f64 - first_count as f64).abs() < 4096.0,
        "44.1k song produced {first_count} samples (expected ~96000)"
    );
    assert!(
        (96_000.0_f64 - second_count as f64).abs() < 4096.0,
        "96k song produced {second_count} samples (expected ~96000)"
    );
}
