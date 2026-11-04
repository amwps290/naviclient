//! Audio worker command processing, stream preparation, and playback state polling.

use super::*;

const STALL_VISIBLE_AFTER: Duration = Duration::from_secs(3);
const HTTP_IO_TIMEOUT: Duration = Duration::from_secs(12);
const SEEK_DEBOUNCE: Duration = Duration::from_millis(180);

#[derive(Debug)]
pub(super) enum Command {
    Play {
        url: String,
        cache_key: String,
        duration_hint: Option<Duration>,
        start_position: Duration,
        /// 该曲目的响度补偿增益系数（1.0 = 不变），与用户音量相乘。
        gain: f32,
        preload_id: Option<u64>,
    },
    Preload {
        id: u64,
        url: String,
        cache_key: String,
        duration_hint: Option<Duration>,
        gain: f32,
        transition: Arc<Transition>,
    },
    CancelPreload,
    Pause,
    Resume,
    Stop,
    SetVolume(f32),
    SetCacheDirectory(PathBuf),
    /// 切换输出设备；None 表示跟随系统默认。
    SetOutputDevice(Option<String>),
    /// 更新均衡器设置。
    SetEqualizer(EqSettings),
    Seek(Duration),
}

pub(super) fn apply_playback_intent(player: &Player, paused: bool) {
    if paused {
        player.pause();
    } else {
        player.play();
    }
}

pub(super) fn coalesce_seek_commands(
    initial: Duration,
    rx: &Receiver<Command>,
    pending_commands: &mut VecDeque<Command>,
) -> (Duration, usize) {
    let mut latest = initial;
    let mut coalesced = 1;
    let mut deadline = Instant::now() + SEEK_DEBOUNCE;

    while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(remaining) {
            Ok(Command::Seek(position)) => {
                latest = position;
                coalesced += 1;
                deadline = Instant::now() + SEEK_DEBOUNCE;
            }
            Ok(command) => {
                pending_commands.push_back(command);
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout | mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    (latest, coalesced)
}

#[derive(Clone, Copy, Debug)]
enum PreparationKind {
    Play,
    Seek,
}

struct PreparationResult {
    generation: u64,
    kind: PreparationKind,
    start_position: Duration,
    result: Result<PreparedStream>,
}

#[allow(clippy::too_many_arguments)]
fn spawn_preparation(
    tx: Sender<PreparationResult>,
    generation: u64,
    kind: PreparationKind,
    client: Client,
    url: String,
    cache_dir: PathBuf,
    cache_key: String,
    duration_hint: Option<Duration>,
    start_position: Duration,
) {
    let endpoint = stream_endpoint(&url);
    let spawn_result = thread::Builder::new()
        .name("audio-prepare".to_string())
        .spawn(move || {
            let result = play_url(
                client,
                &url,
                &cache_dir,
                &cache_key,
                duration_hint,
                start_position,
                StreamControl::new(false),
            );
            let _ = tx.send(PreparationResult {
                generation,
                kind,
                start_position,
                result,
            });
        });
    if let Err(error) = spawn_result {
        log::error!("failed to start audio preparation thread; endpoint={endpoint}: {error}");
    }
}

pub(super) fn run_worker(
    rx: Receiver<Command>,
    state: Arc<Mutex<PlaybackState>>,
    initial_cache_dir: PathBuf,
    output_device: Option<String>,
) {
    let (device_sink, output_name) = match open_audio_device(output_device.as_deref()) {
        Ok(opened) => opened,
        Err(error) => {
            log::error!("failed to open the audio output device: {error:#}");
            set_state(&state, |s| {
                s.active = false;
                s.ended = false;
                s.error = Some(
                    t!(
                        "Failed to open the audio output device: %{error}",
                        error = format!("{error:#}")
                    )
                    .to_string(),
                );
            });
            return;
        }
    };
    log::info!(
        "audio output device opened; name={:?} channels={} sample_rate={}",
        output_name,
        device_sink.config().channel_count().get(),
        device_sink.config().sample_rate().get()
    );
    report_output_device(&state, &output_name, &device_sink);

    let client = match Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(HTTP_IO_TIMEOUT)
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            log::error!("failed to create the audio HTTP client: {error}");
            set_state(&state, |s| {
                s.active = false;
                s.ended = false;
                s.error = Some(
                    t!(
                        "Failed to create the audio HTTP client: %{error}",
                        error = error
                    )
                    .to_string(),
                );
            });
            return;
        }
    };

    let output_channels = device_sink.config().channel_count();
    let output_sample_rate = device_sink.config().sample_rate();
    let mixer = device_sink.mixer().clone();
    let (player, source) = Player::new();
    mixer.add(source);
    let output = AudioOutput {
        sink: Some(device_sink),
        name: output_name,
        player,
        channels: output_channels,
        sample_rate: output_sample_rate,
        handoff: Arc::new(Mutex::new(None)),
        eq: new_eq_handle(EqSettings::default()),
    };
    run_playback(rx, state, initial_cache_dir, client, output);
}

pub(super) fn run_playback(
    rx: Receiver<Command>,
    state: Arc<Mutex<PlaybackState>>,
    initial_cache_dir: PathBuf,
    client: Client,
    output: AudioOutput,
) {
    // sink_keeper/output_name 的"读取"就是保活副作用：变量本身从不被读，
    // 设备切换时被整体替换，旧值 drop 即关闭旧输出流。
    #[allow(unused_assignments)]
    let AudioOutput {
        sink: mut sink_keeper,
        name: mut output_name,
        mut player,
        channels: mut output_channels,
        sample_rate: mut output_sample_rate,
        handoff,
        eq: eq_shared,
    } = output;
    let (preparation_tx, preparation_rx) = mpsc::channel::<PreparationResult>();
    let mut preloader = Preloader::new(client.clone())
        .map_err(|error| {
            log::warn!("next-track preloading unavailable: {error:#}");
            error
        })
        .ok();

    let mut volume = 1.0;
    let mut current_gain = 1.0;
    let mut loudness_shared = Arc::new(Mutex::new(None::<(f32, f32)>));
    let mut duration: Option<Duration> = None;
    let mut stream_error: Option<SharedStreamError> = None;
    let mut buffer_progress: Option<Arc<BufferProgress>> = None;
    let mut decode_progress: Option<Arc<StreamControl>> = None;
    let mut startup_error: Option<FailureError> = None;
    let mut cache_dir = initial_cache_dir;
    if let Err(error) = prepare_cache_dir(&cache_dir) {
        log::warn!("audio cache is unavailable: {error:#}");
    } else {
        log::debug!("audio cache ready; directory={}", cache_dir.display());
    }

    let mut current_url: Option<String> = None;
    let mut current_cache_key: Option<String> = None;
    let mut position_offset = Duration::ZERO;
    let mut terminal_state_logged = true;
    let mut pending_commands = VecDeque::new();
    let mut preparation_generation = 0_u64;
    let mut preparing = false;
    // 最近一次进入「准备播放」的时刻，用于在启动缓冲阶段显示等待时长。
    let mut preparing_since: Option<Instant> = None;
    let mut desired_paused = false;
    let mut pending_preload = None;
    let mut next_request = None;
    let mut armed: Option<AutoTrack> = None;
    let mut track_boundary = Duration::ZERO;
    let mut transition_id = None;

    loop {
        let command = if let Some(command) = pending_commands.pop_front() {
            Some(command)
        } else {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(command) => Some(command),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        };

        if let Some(event) = armed.as_ref().and_then(|next| next.transition.event()) {
            let next = armed.take().unwrap();
            duration = next.info.duration;
            stream_error = Some(next.info.error);
            buffer_progress = Some(next.info.buffer);
            decode_progress = Some(next.info.control);
            loudness_shared = next.info.loudness;
            current_url = Some(next.url);
            current_cache_key = Some(next.cache_key);
            current_gain = next.gain;
            track_boundary = event.boundary;
            position_offset = Duration::ZERO;
            transition_id = Some(event.id);
            terminal_state_logged = false;
            next_request = None;
            log::info!("gapless track started; ticket={}", event.id);
        }

        let mut ready_output = None;
        if let Some(command) = command {
            match command {
                Command::Play {
                    url,
                    cache_key,
                    duration_hint,
                    start_position,
                    gain,
                    preload_id,
                } => {
                    pending_preload = None;
                    next_request = None;
                    armed = None;
                    track_boundary = Duration::ZERO;
                    transition_id = None;
                    let queued = handoff.lock().unwrap().take().filter(|next| {
                        preload_id == Some(next.transition.id)
                            && next.cache_key == cache_key
                            && start_position.is_zero()
                    });
                    log::info!(
                        "play requested; endpoint={} duration_hint_ms={:?} gain={gain}",
                        stream_endpoint(&url),
                        duration_hint.map(|duration| duration.as_millis())
                    );
                    current_gain = gain.clamp(0.1, 4.0);
                    preparation_generation = preparation_generation.wrapping_add(1);
                    preparing = true;
                    preparing_since = Some(Instant::now());
                    desired_paused = false;
                    terminal_state_logged = false;
                    player.stop();
                    player.clear();
                    duration = duration_hint;
                    stream_error = None;
                    buffer_progress = None;
                    decode_progress = None;
                    startup_error = None;
                    current_url = Some(url.clone());
                    current_cache_key = Some(cache_key.clone());
                    let start_position =
                        duration_hint.map_or(start_position, |limit| start_position.min(limit));
                    position_offset = start_position;
                    set_state(&state, |s| {
                        s.active = true;
                        s.paused = false;
                        s.ended = false;
                        s.position = start_position;
                        s.buffered = Duration::ZERO;
                        s.fully_buffered = false;
                        s.transition_id = None;
                        s.duration = duration_hint;
                        s.error = None;
                    });
                    let preloaded = preloader.as_mut().and_then(|preloader| {
                        preloader.take(preload_id, &cache_key, start_position)
                    });
                    if let Some(queued) = queued {
                        queued.track.info.control.promote();
                        ready_output = Some((PreparationKind::Play, start_position, queued.track));
                    } else if let Some(stream) = preloaded {
                        let _ = preparation_tx.send(PreparationResult {
                            generation: preparation_generation,
                            kind: PreparationKind::Play,
                            start_position,
                            result: Ok(stream),
                        });
                    } else {
                        spawn_preparation(
                            preparation_tx.clone(),
                            preparation_generation,
                            PreparationKind::Play,
                            client.clone(),
                            url,
                            cache_dir.clone(),
                            cache_key,
                            duration_hint,
                            start_position,
                        );
                    }
                }
                Command::Preload {
                    id,
                    url,
                    cache_key,
                    duration_hint,
                    gain,
                    transition,
                } => {
                    pending_preload = Some((id, url, cache_key, duration_hint, gain, transition));
                }
                Command::CancelPreload => {
                    handoff.lock().unwrap().take();
                    next_request = None;
                    armed = None;
                    pending_preload = None;
                    if let Some(preloader) = preloader.as_mut() {
                        preloader.cancel();
                    }
                }
                Command::Pause => {
                    log::debug!("playback pause requested");
                    desired_paused = true;
                    player.pause();
                    set_state(&state, |s| s.paused = true);
                }
                Command::Resume => {
                    log::debug!("playback resume requested");
                    desired_paused = false;
                    player.play();
                    set_state(&state, |s| s.paused = false);
                }
                Command::Stop => {
                    handoff.lock().unwrap().take();
                    next_request = None;
                    armed = None;
                    track_boundary = Duration::ZERO;
                    transition_id = None;
                    pending_preload = None;
                    if let Some(preloader) = preloader.as_mut() {
                        preloader.cancel();
                    }
                    log::debug!("playback stop requested");
                    preparation_generation = preparation_generation.wrapping_add(1);
                    preparing = false;
                    preparing_since = None;
                    desired_paused = false;
                    terminal_state_logged = true;
                    player.stop();
                    player.clear();
                    duration = None;
                    stream_error = None;
                    buffer_progress = None;
                    decode_progress = None;
                    startup_error = None;
                    current_url = None;
                    current_cache_key = None;
                    position_offset = Duration::ZERO;
                    set_state(&state, |s| {
                        s.active = false;
                        s.paused = false;
                        s.ended = true;
                        s.position = Duration::ZERO;
                        s.buffered = Duration::ZERO;
                        s.duration = None;
                        s.error = None;
                    });
                }
                Command::SetVolume(new_volume) => {
                    volume = new_volume.clamp(0.0, 1.0);
                    player.set_volume(volume);
                }
                Command::SetCacheDirectory(new_cache_dir) => {
                    handoff.lock().unwrap().take();
                    next_request = None;
                    armed = None;
                    pending_preload = None;
                    if let Some(preloader) = preloader.as_mut() {
                        preloader.cancel();
                    }
                    match prepare_cache_dir(&new_cache_dir) {
                        Ok(()) => {
                            log::info!(
                                "audio cache directory changed; directory={}",
                                new_cache_dir.display()
                            );
                            cache_dir = new_cache_dir;
                        }
                        Err(error) => log::error!(
                            "failed to change audio cache directory; directory={}: {error:#}",
                            new_cache_dir.display()
                        ),
                    }
                }
                Command::SetOutputDevice(target) => {
                    // 先记录当前输出进度与出声状态（旧 Player 释放前）。
                    let had_output = !player.empty();
                    let synthetic_silence = decode_progress
                        .as_ref()
                        .map(|control| control.synthetic_silence_duration())
                        .unwrap_or_default();
                    let position = logical_playback_position(
                        position_offset,
                        player.get_pos().saturating_sub(track_boundary),
                        synthetic_silence,
                    );
                    match open_audio_device(target.as_deref()) {
                        Ok((mut new_sink, new_name)) => {
                            let channels = new_sink.config().channel_count();
                            let sample_rate = new_sink.config().sample_rate();
                            let mixer = new_sink.mixer().clone();
                            let (new_player, source) = Player::new();
                            mixer.add(source);
                            new_sink.log_on_drop(false);
                            log::info!(
                                "audio output switched; name={:?} channels={} sample_rate={}",
                                new_name,
                                channels.get(),
                                sample_rate.get()
                            );
                            // 替换保活变量：旧 sink 与旧 Player 上的源一起释放。
                            let new_sink_ref = sink_keeper.insert(new_sink);
                            output_name = new_name;
                            player = new_player;
                            output_channels = channels;
                            output_sample_rate = sample_rate;
                            report_output_device(&state, &output_name, new_sink_ref);
                            // 正在出声的曲目：在新输出上从当前位置继续。
                            // 复用 Seek 路径重建流（保留暂停意图、作废预
                            // 加载衔接）；准备中的曲目会在新 Player 上落地。
                            if had_output {
                                pending_commands.push_back(Command::Seek(position));
                            }
                        }
                        Err(error) => {
                            // 保留原输出继续播放；空闲时错误会经由启动错误显示。
                            log::warn!("failed to switch audio output to {target:?}: {error:#}");
                            startup_error = Some(FailureError::new(
                                StreamFailure::Output,
                                t!(
                                    "unable to switch audio output: %{error}",
                                    error = format!("{error:#}")
                                )
                                .to_string(),
                            ));
                        }
                    }
                }
                Command::SetEqualizer(settings) => {
                    // 共享状态 + 版本号：滤波器在下一个检查点重算系数。
                    if let Ok(mut guard) = eq_shared.lock() {
                        guard.settings = settings;
                        guard.version = guard.version.wrapping_add(1);
                    }
                }
                Command::Seek(position) => {
                    handoff.lock().unwrap().take();
                    next_request = None;
                    armed = None;
                    track_boundary = Duration::ZERO;
                    pending_preload = None;
                    if let Some(preloader) = preloader.as_mut() {
                        preloader.cancel();
                    }
                    let (position, coalesced) =
                        coalesce_seek_commands(position, &rx, &mut pending_commands);
                    let (Some(url), Some(cache_key)) =
                        (current_url.clone(), current_cache_key.clone())
                    else {
                        continue;
                    };
                    let target = duration
                        .map(|duration| position.min(duration))
                        .unwrap_or(position);
                    log::info!(
                        "playback seek requested; target_ms={} coalesced_events={coalesced}",
                        target.as_millis()
                    );
                    preparation_generation = preparation_generation.wrapping_add(1);
                    preparing = true;
                    preparing_since = Some(Instant::now());
                    // clear()/stop() can pause the sink during preparation. Only
                    // explicit user commands determine the intended transport state.
                    terminal_state_logged = false;
                    player.stop();
                    player.clear();
                    stream_error = None;
                    buffer_progress = None;
                    decode_progress = None;
                    startup_error = None;
                    position_offset = target;
                    set_state(&state, |s| {
                        s.active = true;
                        s.paused = desired_paused;
                        s.ended = false;
                        s.position = target;
                        s.error = None;
                    });
                    spawn_preparation(
                        preparation_tx.clone(),
                        preparation_generation,
                        PreparationKind::Seek,
                        client.clone(),
                        url,
                        cache_dir.clone(),
                        cache_key,
                        duration,
                        target,
                    );
                }
            }
        }

        while let Ok(prepared) = preparation_rx.try_recv() {
            if prepared.generation != preparation_generation {
                log::debug!(
                    "discarding stale audio preparation; generation={} current_generation={} kind={:?}",
                    prepared.generation,
                    preparation_generation,
                    prepared.kind
                );
                continue;
            }
            preparing = false;
            match prepared.result {
                Ok(stream) => {
                    ready_output = Some((
                        prepared.kind,
                        prepared.start_position,
                        OutputTrack::new(
                            stream,
                            current_gain,
                            output_channels,
                            output_sample_rate,
                            eq_shared.clone(),
                        ),
                    ));
                }
                Err(error) => {
                    stream_error = None;
                    buffer_progress = None;
                    let kind = classify_anyhow(&error);
                    let detail = match prepared.kind {
                        PreparationKind::Play => t!(
                            "could not start playback: %{error}",
                            error = format!("{error:#}")
                        )
                        .to_string(),
                        PreparationKind::Seek => {
                            t!("could not seek: %{error}", error = format!("{error:#}")).to_string()
                        }
                    };
                    log::warn!(
                        "audio preparation failed; kind={:?}: {error:#}",
                        prepared.kind
                    );
                    let failure = FailureError::new(kind, detail);
                    let message = failure.message();
                    startup_error = Some(failure);
                    set_state(&state, |s| {
                        s.active = false;
                        s.ended = false;
                        s.error = Some(message);
                    });
                }
            }
        }

        if let Some((kind, start_position, track)) = ready_output {
            preparing = false;
            preparing_since = None;
            duration = track.info.duration;
            stream_error = Some(track.info.error.clone());
            buffer_progress = Some(track.info.buffer.clone());
            decode_progress = Some(track.info.control.clone());
            loudness_shared = track.info.loudness.clone();
            player.set_volume(volume);
            apply_playback_intent(&player, desired_paused);
            player.append(GaplessSource::new(
                track,
                handoff.clone(),
                output_channels,
                output_sample_rate,
            ));
            log::info!(
                "playback prepared; kind={kind:?} start_ms={}",
                start_position.as_millis()
            );
            set_state(&state, |s| {
                s.active = true;
                s.paused = desired_paused;
                s.ended = false;
                s.position = start_position;
                if matches!(kind, PreparationKind::Seek) {
                    s.seek_serial = s.seek_serial.wrapping_add(1);
                }
                s.duration = duration;
                s.error = None;
            });
        }

        if let Some(preloader) = preloader.as_mut() {
            preloader.poll();
        }

        let output_position = player.get_pos().saturating_sub(track_boundary);
        let synthetic_silence = decode_progress
            .as_ref()
            .map(|control| control.synthetic_silence_duration())
            .unwrap_or_default();
        let position =
            logical_playback_position(position_offset, output_position, synthetic_silence);
        let fully_buffered = decode_progress
            .as_ref()
            .is_some_and(|control| control.fully_decoded.load(Ordering::Acquire));
        // A UI request may arrive while a seek is still preparing. Keep its
        // reservation, but defer network I/O until the current audio is safe.
        let can_preload = !preparing
            && !player.empty()
            && !desired_paused
            && fully_buffered
            && duration.is_some_and(|duration| {
                let remaining = duration.saturating_sub(position);
                !remaining.is_zero() && remaining <= Duration::from_secs(15)
            });
        if can_preload {
            if let Some((id, url, cache_key, duration_hint, gain, transition)) =
                pending_preload.take()
            {
                if let Some(preloader) = preloader.as_mut().filter(|_| transition.pending()) {
                    preloader.request(
                        id,
                        url.clone(),
                        cache_key.clone(),
                        duration_hint,
                        cache_dir.clone(),
                    );
                    next_request = Some((url, cache_key, gain, transition));
                }
            }
        }
        if armed.is_none() {
            if let Some((url, cache_key, gain, transition)) = next_request.as_ref() {
                if transition.pending() {
                    if let Some(stream) = preloader.as_mut().and_then(Preloader::take_ready) {
                        let track = OutputTrack::new(
                            stream,
                            *gain,
                            output_channels,
                            output_sample_rate,
                            eq_shared.clone(),
                        );
                        armed = Some(AutoTrack {
                            transition: transition.clone(),
                            url: url.clone(),
                            cache_key: cache_key.clone(),
                            gain: *gain,
                            info: track.info.clone(),
                        });
                        *handoff.lock().unwrap() = Some(ArmedTrack {
                            transition: transition.clone(),
                            cache_key: cache_key.clone(),
                            track,
                        });
                    }
                }
            }
        }
        let paused = if preparing {
            desired_paused
        } else {
            player.is_paused()
        };
        let empty = player.empty();
        let error = if empty && !preparing {
            startup_error.clone().or_else(|| {
                stream_error
                    .as_ref()
                    .and_then(|error| error.lock().ok()?.clone())
            })
        } else {
            None
        };
        let failure = error.as_ref().map(|failure| failure.kind);
        let error = error.map(|failure| failure.message());
        // An idle sink after launch/Stop is not a track reaching its end. In
        // particular, restoring a queue must not automatically skip and play it.
        let ended = current_url.is_some() && empty && !preparing && error.is_none();
        if empty && !preparing && !terminal_state_logged {
            if let Some(error) = error.as_deref() {
                log::error!("playback stopped with error: {error}");
            } else {
                log::info!("playback reached end; position_ms={}", position.as_millis());
            }
            terminal_state_logged = true;
        }
        let buffered = duration
            .zip(buffer_progress.as_ref())
            .map(|(duration, progress)| progress.buffered_duration(duration))
            .unwrap_or_default();
        // 数据不再到达时界面要能看出「在等网络」而不是「卡死」。只在确实
        // 播放中、且本地缓冲已耗尽时才算停滞。
        // 只按整秒对外暴露，避免界面因为毫秒级变化而整窗重绘。
        let stall = if preparing {
            // 启动缓冲：只要拉流迟迟没就绪就让界面显示等待时长，而不是一片寂静。
            preparing_since
                .map(|since| since.elapsed())
                .filter(|elapsed| *elapsed >= STALL_VISIBLE_AFTER)
        } else {
            decode_progress.as_ref().and_then(|control| {
                let idle = control.idle();
                (!empty && !paused && idle >= STALL_VISIBLE_AFTER).then_some(idle)
            })
        }
        .map(|elapsed| Duration::from_secs(elapsed.as_secs()));
        let (loudness_db, loudness_peak) = loudness_shared
            .lock()
            .ok()
            .and_then(|shared| *shared)
            .unwrap_or((0.0, 0.0));
        set_state(&state, |s| {
            // 元数据时长和解码时长可能有少量舍入差异；对外状态始终保持
            // position <= duration，时间文本、进度条和媒体会话读取同一值。
            s.position = duration.map_or(position, |limit| position.min(limit));
            s.buffered = buffered;
            s.fully_buffered = fully_buffered;
            s.transition_id = transition_id;
            s.duration = duration;
            s.paused = paused;
            s.ended = ended;
            s.active = preparing || !empty;
            s.error = error;
            s.failure = failure;
            s.stall = stall;
            s.loudness_db = loudness_db;
            s.loudness_peak = loudness_peak;
        });
    }
}
