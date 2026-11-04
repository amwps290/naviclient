use super::*;
use rodio::source::UniformSourceIterator;

#[test]
fn subsonic_error_envelope_yields_the_server_message() {
    // 标准错误信封：取出服务端给出的真实原因与错误码。
    let body = r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":0,"message":"transcoding not configured"}}}"#;
    assert_eq!(
        subsonic_error_detail(body),
        "transcoding not configured (Subsonic error 0)"
    );
    // 只有错误码：仍可给出可辨识的描述。
    assert_eq!(
        subsonic_error_detail(r#"{"error":{"code":70}}"#),
        "Subsonic error 70"
    );
    // JSON 但非信封结构：回退原始正文。
    let gateway = r#"{"detail":"gateway panic"}"#;
    assert_eq!(subsonic_error_detail(gateway), gateway);
}

#[test]
fn subsonic_error_envelope_falls_back_to_raw_text_when_unparseable() {
    // 非 JSON 正文：截断保留前 200 字符。
    let html = "<html>502 Bad Gateway</html>";
    assert_eq!(subsonic_error_detail(html), html);
    let long = "x".repeat(300);
    assert_eq!(
        subsonic_error_detail(&long),
        format!("{}…", "x".repeat(200))
    );
    // 空正文：给出占位描述而不是空字符串。
    assert_eq!(
        subsonic_error_detail(""),
        "the server returned an empty response"
    );
    assert_eq!(
        subsonic_error_detail("   "),
        "the server returned an empty response"
    );
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
    };
    audio.play(
        "test".into(),
        "song".into(),
        Some(Duration::from_secs(180)),
        1.0,
        Duration::from_secs(42),
        None,
    );
    assert!(
        matches!(rx.recv().unwrap(), Command::Play { start_position, .. } if start_position == Duration::from_secs(42))
    );
    assert!(rx.try_recv().is_err());
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
    let (sender, receiver) = mpsc::channel();
    sender.send(Command::Seek(Duration::from_secs(20))).unwrap();
    sender.send(Command::Seek(Duration::from_secs(30))).unwrap();
    sender.send(Command::Pause).unwrap();

    let first = receiver.recv().unwrap();
    let Command::Seek(initial) = first else {
        panic!("expected seek command");
    };
    let mut pending = VecDeque::new();
    let (latest, count) = coalesce_seek_commands(initial, &receiver, &mut pending);

    assert_eq!(latest, Duration::from_secs(30));
    assert_eq!(count, 2);
    assert!(matches!(pending.pop_front(), Some(Command::Pause)));
}

#[test]
fn seek_coalescing_preserves_intervening_transport_commands() {
    let (tx, rx) = mpsc::channel();
    tx.send(Command::Pause).unwrap();
    tx.send(Command::Seek(Duration::from_secs(30))).unwrap();
    let mut pending = VecDeque::new();
    let (position, count) = coalesce_seek_commands(Duration::from_secs(10), &rx, &mut pending);
    assert_eq!((position, count), (Duration::from_secs(10), 1));
    assert!(matches!(pending.pop_front(), Some(Command::Pause)));
    assert!(
        matches!(rx.recv().unwrap(), Command::Seek(position) if position == Duration::from_secs(30))
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
fn buffer_progress_maps_cached_bytes_to_duration() {
    let progress = BufferProgress::new(25, Some(100));
    assert_eq!(
        progress.buffered_duration(Duration::from_secs(200)),
        Duration::from_secs(50)
    );
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
fn completed_cache_is_read_without_network_access() {
    let cache_dir = std::env::temp_dir().join(format!(
        "navidrome-audio-cache-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&cache_dir).unwrap();
    let (complete_path, _) = cache_paths(&cache_dir, "server:song-1");
    fs::write(&complete_path, b"cached audio bytes").unwrap();

    let client = Client::new();
    let mut reader = HttpRangeReader::open(
        client,
        "http://127.0.0.1:1/unreachable".to_string(),
        &cache_dir,
        "server:song-1",
    )
    .unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();

    assert_eq!(bytes, b"cached audio bytes");
    drop(reader);
    fs::remove_dir_all(cache_dir).unwrap();
}

#[test]
fn cache_write_failure_falls_back_to_network_playback() {
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request).unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\naudio")
            .unwrap();
    });
    let invalid_cache_dir = std::env::temp_dir().join(format!(
        "navidrome-audio-cache-file-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::write(&invalid_cache_dir, b"not a directory").unwrap();

    let mut reader = HttpRangeReader::open(
        Client::new(),
        format!("http://{address}/audio"),
        &invalid_cache_dir,
        "server:song-1",
    )
    .unwrap();
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).unwrap();

    assert_eq!(bytes, b"audio");
    drop(reader);
    server.join().unwrap();
    fs::remove_file(invalid_cache_dir).unwrap();
}

#[test]
fn accepts_small_transcode_length_estimation_errors_only_near_the_end() {
    assert!(near_estimated_end(9_399_881, 9_623_552));
    assert!(near_estimated_end(95, 100));
    assert!(!near_estimated_end(94, 100));
    assert!(!near_estimated_end(0, 0));
}

#[test]
fn detects_transcoded_streams_without_logging_authentication_data() {
    assert!(is_transcoded_stream(
        "https://example.test/rest/stream.view?id=song-1&maxBitRate=320&t=secret"
    ));
    assert!(!is_transcoded_stream(
        "https://example.test/rest/stream.view?id=song-1&t=secret"
    ));
    assert!(!is_transcoded_stream("not a url"));
}

#[test]
fn cache_paths_are_stable_and_server_specific() {
    let cache_dir = Path::new("cache");
    let first = cache_paths(cache_dir, "https://one.example:song-1");
    let same = cache_paths(cache_dir, "https://one.example:song-1");
    let other_server = cache_paths(cache_dir, "https://two.example:song-1");

    assert_eq!(first, same);
    assert_ne!(first, other_server);
    assert_eq!(
        first.0.extension().and_then(|value| value.to_str()),
        Some("audio")
    );
    assert_eq!(
        first.1.extension().and_then(|value| value.to_str()),
        Some("part")
    );
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

#[test]
fn temporary_decoder_starvation_outputs_silence_instead_of_ending() {
    let (sender, receiver) = mpsc::sync_channel(1);
    let shared_error = Arc::new(Mutex::new(None));
    let mut source = BufferedStreamSource {
        control: StreamControl::new(false),
        receiver,
        queued: VecDeque::new(),
        current: Vec::new().into_iter(),
        channels: ChannelCount::new(2).unwrap(),
        sample_rate: SampleRate::new(48_000).unwrap(),
        duration: Some(Duration::from_secs(180)),
        producer_finished: false,
        terminal_error: None,
        shared_error: Arc::clone(&shared_error),
        last_data: Instant::now(),
        silence_remaining: 0,
    };

    assert_eq!(source.next(), Some(0.0));
    assert_eq!(source.next(), Some(0.0));
    assert!(shared_error.lock().unwrap().is_none());
    drop(sender);
}
