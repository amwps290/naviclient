use super::*;
use crate::audio::preload::tests::{client, wait_for, wav, TestCache};

fn stereo() -> ChannelCount {
    ChannelCount::new(2).unwrap()
}
fn rate(hz: u32) -> SampleRate {
    SampleRate::new(hz).unwrap()
}

fn track(frames: usize, hz: u32, channels: u16, value: f32, gain: f32) -> OutputTrack {
    let control = StreamControl::new(true);
    let (_, receiver) = mpsc::channel();
    let error = Arc::new(Mutex::new(None));
    let duration = Duration::from_secs_f64(frames as f64 / hz as f64);
    let source = BufferedStreamSource {
        control,
        receiver,
        queued: VecDeque::from([vec![value; frames * channels as usize]]),
        current: Vec::new().into_iter(),
        channels: ChannelCount::new(channels).unwrap(),
        sample_rate: rate(hz),
        duration: Some(duration),
        producer_finished: true,
        terminal_error: None,
        shared_error: error.clone(),
        last_data: Instant::now(),
        silence_remaining: 0,
    };
    OutputTrack::new(
        PreparedStream {
            source,
            duration: Some(duration),
            error,
            buffer_progress: Arc::new(BufferProgress::new(1, Some(1))),
        },
        gain,
        stereo(),
        rate(48_000),
    )
}

fn arm(slot: &Slot, id: u64, track: OutputTrack) -> Arc<Transition> {
    let transition = Transition::new(id, track.info.duration);
    *slot.lock().unwrap() = Some(ArmedTrack {
        transition: transition.clone(),
        cache_key: id.to_string(),
        track,
    });
    transition
}

#[test]
fn track_boundary_has_no_added_or_missing_samples_and_keeps_gains_separate() {
    let slot = Slot::default();
    let next = arm(&slot, 1, track(6400, 48_000, 2, 0.5, 0.5));
    let mut output = GaplessSource::new(
        track(4800, 48_000, 2, 0.25, 2.0),
        slot,
        stereo(),
        rate(48_000),
    );
    let first: Vec<_> = output.by_ref().take(9600).collect();
    assert_eq!(first, vec![0.5; 9600]);
    assert!(next.event().is_none());
    let second: Vec<_> = output.collect();
    assert_eq!(second, vec![0.25; 12800]);
    let event = next.take_event().unwrap();
    assert_eq!(event.boundary, Duration::from_millis(100));
    assert!((event.loudness_db - 20.0 * 0.25_f32.log10()).abs() < 0.01);
    assert_eq!(event.loudness_peak, 0.25);
    assert!(next.take_event().is_none());
}

#[test]
fn different_rates_and_channels_match_individually_normalized_audio() {
    let first = track(44_100, 44_100, 1, 0.25, 1.0)
        .source
        .collect::<Vec<_>>();
    let second = track(96_000, 96_000, 2, -0.5, 1.0)
        .source
        .collect::<Vec<_>>();
    assert!((first.len() as isize - 96_000).abs() <= 4);
    assert!((second.len() as isize - 96_000).abs() <= 4);
    let slot = Slot::default();
    arm(&slot, 1, track(96_000, 96_000, 2, -0.5, 1.0));
    let output = GaplessSource::new(
        track(44_100, 44_100, 1, 0.25, 1.0),
        slot,
        stereo(),
        rate(48_000),
    );
    assert_eq!(output.collect::<Vec<_>>(), [first, second].concat());
}

#[test]
fn cancellation_before_boundary_skips_the_stale_track_and_late_preparation_cannot_restart_source() {
    let slot = Slot::default();
    let next = arm(&slot, 1, track(100, 48_000, 2, -0.5, 1.0));
    let mut output = GaplessSource::new(
        track(100, 48_000, 2, 0.25, 1.0),
        slot.clone(),
        stereo(),
        rate(48_000),
    );
    next.cancel();
    assert_eq!(output.by_ref().collect::<Vec<_>>(), vec![0.25; 200]);
    assert!(next.event().is_none());
    let late = arm(&slot, 2, track(100, 48_000, 2, 0.5, 1.0));
    assert_eq!(output.next(), None);
    assert!(late.event().is_none());
}

#[test]
fn cancellation_after_boundary_reports_the_transition_once_without_stopping_current_audio() {
    let slot = Slot::default();
    let next = arm(&slot, 2, track(100, 48_000, 2, -0.5, 1.0));
    let mut output =
        GaplessSource::new(track(1, 48_000, 2, 0.25, 1.0), slot, stereo(), rate(48_000));
    assert_eq!(output.by_ref().take(2).count(), 2);
    assert_eq!(output.next(), Some(-0.5));
    next.cancel();
    assert_eq!(next.take_event().unwrap().id, 2);
    assert!(next.take_event().is_none());
    assert_eq!(output.collect::<Vec<_>>(), vec![-0.5; 199]);
}

#[test]
fn racing_cancel_and_first_sample_have_one_consistent_winner() {
    for _ in 0..32 {
        let slot = Slot::default();
        let next = arm(&slot, 1, track(10, 48_000, 2, -0.5, 1.0));
        let mut output =
            GaplessSource::new(track(1, 48_000, 2, 0.25, 1.0), slot, stereo(), rate(48_000));
        assert_eq!(output.by_ref().take(2).count(), 2);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let worker_barrier = barrier.clone();
        let worker = thread::spawn(move || {
            worker_barrier.wait();
            output.next()
        });
        barrier.wait();
        next.cancel();
        let sample = worker.join().unwrap();
        assert_eq!(sample.is_some(), next.event().is_some());
        assert!(matches!(sample, None | Some(-0.5)));
    }
}

#[test]
fn a_failed_current_stream_does_not_autoplay_the_prepared_track() {
    let slot = Slot::default();
    let next = arm(&slot, 1, track(100, 48_000, 2, -0.5, 1.0));
    let first = track(100, 48_000, 2, 0.25, 1.0);
    *first.info.error.lock().unwrap() = Some("network truncated".into());
    let output = GaplessSource::new(first, slot, stereo(), rate(48_000));
    assert_eq!(output.count(), 200);
    assert!(next.event().is_none());
}

#[test]
fn player_pause_does_not_consume_or_start_the_prepared_song() {
    let slot = Slot::default();
    let next = arm(&slot, 1, track(4800, 48_000, 2, -0.5, 1.0));
    let source = GaplessSource::new(
        track(4800, 48_000, 2, 0.25, 1.0),
        slot,
        stereo(),
        rate(48_000),
    );
    let (player, mut output) = Player::new();
    player.pause();
    player.append(source);
    assert!(output.by_ref().take(20_000).all(|sample| sample == 0.0));
    assert!(next.event().is_none());
    player.play();
    let samples: Vec<_> = output
        .skip_while(|sample| *sample == 0.0)
        .take(19_200)
        .collect();
    assert_eq!(&samples[..9600], &[0.25; 9600]);
    assert_eq!(&samples[9600..], &[-0.5; 9600]);
    assert!(next.event().is_some());
}

#[test]
fn boundary_notification_masks_old_worker_snapshot_and_is_consumed_once() {
    let (tx, _rx) = mpsc::channel();
    let next = Transition::new(7, Some(Duration::from_secs(10)));
    let state = Arc::new(Mutex::new(PlaybackState {
        ended: true,
        position: Duration::from_secs(180),
        loudness_db: -8.0,
        seek_serial: 3,
        ..Default::default()
    }));
    let audio = AudioHandle {
        tx,
        state: state.clone(),
        transition: Arc::new(Mutex::new(Some(next.clone()))),
    };
    assert!(next.start(
        Duration::from_secs(180),
        &Arc::new(Mutex::new(Some((-8.0, 0.8))))
    ));
    let snapshot = audio.state();
    assert!(snapshot.active && !snapshot.ended);
    assert_eq!(snapshot.position, Duration::ZERO);
    assert_eq!(snapshot.duration, Some(Duration::from_secs(10)));
    assert_eq!(snapshot.loudness_db, 0.0);
    assert_eq!(snapshot.seek_serial, 3);
    assert_eq!(audio.cancel_preload().unwrap().id, 7);
    assert!(audio.take_transition().is_none());
    *state.lock().unwrap() = PlaybackState {
        transition_id: Some(7),
        position: Duration::from_millis(50),
        ..snapshot
    };
    assert_eq!(audio.state().position, Duration::from_millis(50));
    audio.stop();
    assert!(audio.transition.lock().unwrap().is_none());
}

struct Engine {
    audio: Option<AudioHandle>,
    output: Option<rodio::queue::SourcesQueueOutput>,
    slot: Slot,
    worker: Option<thread::JoinHandle<()>>,
    _cache: TestCache,
}

impl Engine {
    fn new() -> Self {
        let cache = TestCache::new();
        fs::write(cache_paths(&cache.0, "a").0, wav()).unwrap();
        let mut negative = wav();
        for sample in negative[44..].chunks_exact_mut(2) {
            sample.copy_from_slice(&(-8192_i16).to_le_bytes());
        }
        fs::write(cache_paths(&cache.0, "b").0, negative).unwrap();
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(PlaybackState::default()));
        let audio = AudioHandle {
            tx,
            state: state.clone(),
            transition: Arc::new(Mutex::new(None)),
        };
        let (player, output) = Player::new();
        let slot = Slot::default();
        let device = AudioOutput {
            sink: None,
            name: None,
            player,
            channels: stereo(),
            sample_rate: rate(48_000),
            handoff: slot.clone(),
        };
        let path = cache.0.clone();
        let worker = thread::spawn(move || run_playback(rx, state, path, client(), device));
        audio.play(
            "http://127.0.0.1:1/a".into(),
            "a".into(),
            Some(Duration::from_secs(12)),
            1.0,
            Duration::ZERO,
            None,
        );
        wait_for(|| audio.state().fully_buffered);
        Self {
            audio: Some(audio),
            output: Some(output),
            slot,
            worker: Some(worker),
            _cache: cache,
        }
    }

    fn audio(&self) -> &AudioHandle {
        self.audio.as_ref().unwrap()
    }

    fn prepare_next(&self) -> Arc<StreamControl> {
        self.audio().preload(
            1,
            "http://127.0.0.1:1/b".into(),
            "b".into(),
            Some(Duration::from_secs(12)),
            0.5,
        );
        wait_for(|| self.slot.lock().unwrap().is_some());
        self.slot
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .track
            .info
            .control
            .clone()
    }

    fn pump_until(&mut self, mut condition: impl FnMut(&PlaybackState) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !condition(&self.audio().state()) {
            assert!(
                Instant::now() < deadline,
                "audio worker did not reach expected state"
            );
            for _ in 0..1024 {
                self.output.as_mut().unwrap().next();
            }
            thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.audio.take();
        self.output.take();
        self.worker.take().unwrap().join().unwrap();
    }
}

#[test]
fn worker_joins_preloaded_audio_then_seeks_and_stops_the_correct_track() {
    let mut engine = Engine::new();
    engine.prepare_next();
    let output = engine.output.as_mut().unwrap();
    // Inspect the actual Player output, allowing only its initial idle silence.
    assert_eq!(output.find(|sample| *sample != 0.0), Some(0.25));
    assert!(output
        .by_ref()
        .take(12 * 48_000 * 2 - 1)
        .all(|sample| sample == 0.25));
    assert_eq!(output.next(), Some(-0.125));
    let event = engine.audio().take_transition().unwrap();
    assert_eq!(event.id, 1);
    assert!(event.loudness_db < -12.0 && event.loudness_db > -12.1);
    assert!(engine.audio().take_transition().is_none());
    wait_for(|| engine.audio().state.lock().unwrap().transition_id == Some(1));
    assert_eq!(engine.audio().state().seek_serial, 0);
    assert!(engine.audio().state().position < Duration::from_millis(10));
    engine.audio().seek(Duration::from_secs(3));
    engine.pump_until(|state| state.seek_serial == 1 && state.position >= Duration::from_secs(3));
    assert_eq!(
        engine
            .output
            .as_mut()
            .unwrap()
            .find(|sample| *sample != 0.0),
        Some(-0.125)
    );
    engine.audio().stop();
    engine.pump_until(|state| !state.active);
    assert!(!engine.audio().state().ended);
}

#[test]
fn manual_skip_reuses_the_armed_track_without_a_second_decoder() {
    let mut engine = Engine::new();
    let prepared_control = engine.prepare_next();
    assert!(engine.audio().freeze_preload().is_none());
    engine.audio().play(
        "http://127.0.0.1:1/b".into(),
        "b".into(),
        Some(Duration::from_secs(12)),
        0.5,
        Duration::ZERO,
        Some(1),
    );
    engine
        .pump_until(|state| state.active && prepared_control.fully_decoded.load(Ordering::Acquire));
    assert_eq!(
        engine
            .output
            .as_mut()
            .unwrap()
            .find(|sample| *sample != 0.0),
        Some(-0.125)
    );
    assert!(
        prepared_control.check().is_ok(),
        "manual skip discarded the prepared decoder"
    );
    assert!(engine.audio().take_transition().is_none());
}

#[test]
fn queue_cancellation_prevents_the_worker_from_playing_stale_prepared_audio() {
    let mut engine = Engine::new();
    engine.prepare_next();
    assert!(engine.audio().cancel_preload().is_none());
    let output = engine.output.as_mut().unwrap();
    assert_eq!(output.find(|sample| *sample != 0.0), Some(0.25));
    assert!(output
        .by_ref()
        .take(12 * 48_000 * 2 - 1)
        .all(|sample| sample == 0.25));
    assert!(output.by_ref().take(4096).all(|sample| sample == 0.0));
    engine.pump_until(|state| state.ended);
    assert!(engine.audio().take_transition().is_none());
}

#[test]
fn repeated_song_rearms_chain_and_resets_position_for_each_occurrence() {
    let mut engine = Engine::new();
    engine.prepare_next();
    let output = engine.output.as_mut().unwrap();
    assert_eq!(output.find(|sample| *sample != 0.0), Some(0.25));
    for _ in 0..12 * 48_000 * 2 - 1 {
        output.next();
    }
    assert_eq!(output.next(), Some(-0.125));
    assert_eq!(engine.audio().take_transition().unwrap().id, 1);
    wait_for(|| engine.audio().state.lock().unwrap().transition_id == Some(1));

    engine.audio().preload(
        2,
        "http://127.0.0.1:1/b".into(),
        "b".into(),
        Some(Duration::from_secs(12)),
        0.5,
    );
    wait_for(|| engine.slot.lock().unwrap().is_some());
    let output = engine.output.as_mut().unwrap();
    assert!(output
        .by_ref()
        .take(12 * 48_000 * 2 - 1)
        .all(|sample| sample == -0.125));
    assert_eq!(output.next(), Some(-0.125));
    let event = engine.audio().take_transition().unwrap();
    assert_eq!(event.id, 2);
    assert_eq!(event.boundary, Duration::from_secs(24));
    wait_for(|| engine.audio().state.lock().unwrap().transition_id == Some(2));
    assert!(engine.audio().state().position < Duration::from_millis(10));
    assert_eq!(engine.audio().state().seek_serial, 0);
}
