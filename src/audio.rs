use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{anyhow, Context, Result};
use md5::{Digest, Md5};
use reqwest::blocking::{Client, Response};
use reqwest::header::{CONTENT_RANGE, CONTENT_TYPE, RANGE};
use reqwest::StatusCode;
use rodio::{
    cpal::BufferSize, source::UniformSourceIterator, ChannelCount, Decoder, DeviceSinkBuilder,
    Player, Sample, SampleRate, Source,
};
use url::Url;

const STREAM_BUFFER_BYTES: usize = 1024 * 1024;
const DECODED_CHUNK_FRAMES: usize = 4096;
const DECODED_BUFFER_CHUNKS: usize = 512;
const STARTUP_BUFFER_CHUNKS: usize = 16;
const STARTUP_BUFFER_TIMEOUT: Duration = Duration::from_secs(20);
const STREAM_STALL_TIMEOUT: Duration = Duration::from_secs(30);
const HTTP_IO_TIMEOUT: Duration = Duration::from_secs(12);
const OUTPUT_BUFFER_FRAMES: u32 = 4096;
const HTTP_READ_RETRIES: usize = 3;
const TRANSCODE_END_RETRIES: usize = 1;
const SEEK_DEBOUNCE: Duration = Duration::from_millis(180);
const MAX_AUDIO_CACHE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

type SharedStreamError = Arc<Mutex<Option<String>>>;

#[derive(Clone, Debug, Default)]
pub struct PlaybackState {
    pub active: bool,
    pub paused: bool,
    pub ended: bool,
    pub position: Duration,
    pub buffered: Duration,
    pub duration: Option<Duration>,
    pub error: Option<String>,
}

#[derive(Debug)]
enum Command {
    Play {
        url: String,
        cache_key: String,
        duration_hint: Option<Duration>,
    },
    Pause,
    Resume,
    Stop,
    SetVolume(f32),
    SetCacheDirectory(PathBuf),
    Seek(Duration),
}

#[derive(Clone)]
pub struct AudioHandle {
    tx: Sender<Command>,
    state: Arc<Mutex<PlaybackState>>,
}

impl AudioHandle {
    pub fn start(cache_dir: PathBuf) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(PlaybackState::default()));
        let worker_state = Arc::clone(&state);
        thread::Builder::new()
            .name("audio-worker".to_string())
            .spawn(move || run_worker(rx, worker_state, cache_dir))
            .context("failed to start audio worker")?;
        log::debug!("audio worker thread started");
        Ok(Self { tx, state })
    }

    pub fn play(&self, url: String, cache_key: String, duration_hint: Option<Duration>) {
        if self
            .tx
            .send(Command::Play {
                url,
                cache_key,
                duration_hint,
            })
            .is_err()
        {
            log::error!("failed to send play command: audio worker is unavailable");
        }
    }

    pub fn pause(&self) {
        let _ = self.tx.send(Command::Pause);
    }

    pub fn resume(&self) {
        let _ = self.tx.send(Command::Resume);
    }

    pub fn stop(&self) {
        let _ = self.tx.send(Command::Stop);
    }

    pub fn set_volume(&self, volume: f32) {
        let _ = self.tx.send(Command::SetVolume(volume));
    }

    pub fn set_cache_directory(&self, cache_dir: PathBuf) {
        if self.tx.send(Command::SetCacheDirectory(cache_dir)).is_err() {
            log::error!("failed to update cache directory: audio worker is unavailable");
        }
    }

    pub fn seek(&self, position: Duration) {
        let _ = self.tx.send(Command::Seek(position));
    }

    pub fn state(&self) -> PlaybackState {
        self.state
            .lock()
            .map(|state| state.clone())
            .unwrap_or_default()
    }
}

fn coalesce_seek_commands(
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
            Ok(command) => pending_commands.push_back(command),
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
    paused: bool,
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
    paused: bool,
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
            );
            let _ = tx.send(PreparationResult {
                generation,
                kind,
                start_position,
                paused,
                result,
            });
        });
    if let Err(error) = spawn_result {
        log::error!("failed to start audio preparation thread; endpoint={endpoint}: {error}");
    }
}

fn run_worker(rx: Receiver<Command>, state: Arc<Mutex<PlaybackState>>, initial_cache_dir: PathBuf) {
    let mut device_sink = match open_audio_sink() {
        Ok(sink) => sink,
        Err(error) => {
            log::error!("failed to open the default audio output device: {error}");
            set_state(&state, |s| {
                s.active = false;
                s.ended = false;
                s.error = Some(format!(
                    "Failed to open the default audio output device: {error}"
                ));
            });
            return;
        }
    };
    log::info!(
        "default audio output device opened; channels={} sample_rate={}",
        device_sink.config().channel_count().get(),
        device_sink.config().sample_rate().get()
    );

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
                s.error = Some(format!("Failed to create the audio HTTP client: {error}"));
            });
            return;
        }
    };

    device_sink.log_on_drop(false);
    let output_channels = device_sink.config().channel_count();
    let output_sample_rate = device_sink.config().sample_rate();
    let mixer = device_sink.mixer().clone();
    let (player, source) = Player::new();
    mixer.add(source);
    let (preparation_tx, preparation_rx) = mpsc::channel::<PreparationResult>();

    let mut volume = 1.0;
    let mut duration: Option<Duration> = None;
    let mut stream_error: Option<SharedStreamError> = None;
    let mut buffer_progress: Option<Arc<BufferProgress>> = None;
    let mut startup_error: Option<String> = None;
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
    let mut desired_paused = false;

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

        if let Some(command) = command {
            match command {
                Command::Play {
                    url,
                    cache_key,
                    duration_hint,
                } => {
                    log::info!(
                        "play requested; endpoint={} duration_hint_ms={:?}",
                        stream_endpoint(&url),
                        duration_hint.map(|duration| duration.as_millis())
                    );
                    preparation_generation = preparation_generation.wrapping_add(1);
                    preparing = true;
                    desired_paused = false;
                    terminal_state_logged = false;
                    player.stop();
                    player.clear();
                    duration = duration_hint;
                    stream_error = None;
                    buffer_progress = None;
                    startup_error = None;
                    current_url = Some(url.clone());
                    current_cache_key = Some(cache_key.clone());
                    position_offset = Duration::ZERO;
                    set_state(&state, |s| {
                        s.active = true;
                        s.paused = false;
                        s.ended = false;
                        s.position = Duration::ZERO;
                        s.buffered = Duration::ZERO;
                        s.duration = duration_hint;
                        s.error = None;
                    });
                    spawn_preparation(
                        preparation_tx.clone(),
                        preparation_generation,
                        PreparationKind::Play,
                        client.clone(),
                        url,
                        cache_dir.clone(),
                        cache_key,
                        duration_hint,
                        Duration::ZERO,
                        false,
                    );
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
                    log::debug!("playback stop requested");
                    preparation_generation = preparation_generation.wrapping_add(1);
                    preparing = false;
                    desired_paused = false;
                    terminal_state_logged = true;
                    player.stop();
                    player.clear();
                    duration = None;
                    stream_error = None;
                    buffer_progress = None;
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
                Command::Seek(position) => {
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
                    desired_paused = player.is_paused() || desired_paused;
                    terminal_state_logged = false;
                    player.stop();
                    player.clear();
                    stream_error = None;
                    buffer_progress = None;
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
                        desired_paused,
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
                    player.set_volume(volume);
                    player.append(normalize_for_output(
                        stream.source,
                        output_channels,
                        output_sample_rate,
                    ));
                    if prepared.paused {
                        player.pause();
                    } else {
                        player.play();
                    }
                    desired_paused = prepared.paused;
                    duration = stream.duration;
                    stream_error = Some(stream.error);
                    buffer_progress = Some(stream.buffer_progress);
                    match prepared.kind {
                        PreparationKind::Play => log::info!(
                            "playback started; duration_ms={:?}",
                            duration.map(|duration| duration.as_millis())
                        ),
                        PreparationKind::Seek => log::info!(
                            "playback seek completed; target_ms={}",
                            prepared.start_position.as_millis()
                        ),
                    }
                    set_state(&state, |s| {
                        s.active = true;
                        s.paused = prepared.paused;
                        s.ended = false;
                        s.position = prepared.start_position;
                        s.duration = duration;
                        s.error = None;
                    });
                }
                Err(error) => {
                    stream_error = None;
                    buffer_progress = None;
                    let message = match prepared.kind {
                        PreparationKind::Play => {
                            format!("Unable to start playback: {error:#}")
                        }
                        PreparationKind::Seek => {
                            format!("Unable to seek audio stream: {error:#}")
                        }
                    };
                    log::warn!(
                        "audio preparation failed; kind={:?}: {error:#}",
                        prepared.kind
                    );
                    startup_error = Some(message.clone());
                    set_state(&state, |s| {
                        s.active = false;
                        s.ended = false;
                        s.error = Some(message);
                    });
                }
            }
        }

        let position = position_offset.saturating_add(player.get_pos());
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
        let ended = empty && !preparing && error.is_none();
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
        set_state(&state, |s| {
            s.position = position;
            s.buffered = buffered;
            s.duration = duration;
            s.paused = paused;
            s.ended = ended;
            s.active = preparing || !empty;
            s.error = error;
        });
    }
}

fn normalize_for_output<S>(
    source: S,
    channels: ChannelCount,
    sample_rate: SampleRate,
) -> UniformSourceIterator<S>
where
    S: Source,
{
    UniformSourceIterator::new(source, channels, sample_rate)
}

fn open_audio_sink() -> Result<rodio::MixerDeviceSink, rodio::DeviceSinkError> {
    match DeviceSinkBuilder::from_default_device().and_then(|builder| {
        builder
            .with_buffer_size(BufferSize::Fixed(OUTPUT_BUFFER_FRAMES))
            .open_sink_or_fallback()
    }) {
        Ok(sink) => {
            log::debug!("audio output opened with fixed buffer; frames={OUTPUT_BUFFER_FRAMES}");
            Ok(sink)
        }
        Err(error) => {
            log::warn!("fixed-buffer audio output failed, trying system default buffer: {error}");
            DeviceSinkBuilder::open_default_sink()
        }
    }
}

fn play_url(
    client: Client,
    url: &str,
    cache_dir: &Path,
    cache_key: &str,
    duration_hint: Option<Duration>,
    start_position: Duration,
) -> Result<PreparedStream> {
    log::debug!(
        "preparing audio stream; endpoint={} start_ms={}",
        stream_endpoint(url),
        start_position.as_millis()
    );
    let reader = HttpRangeReader::open(client, url.to_string(), cache_dir, cache_key)?;
    let buffer_progress = Arc::clone(&reader.buffer_progress);
    let byte_len = reader.len();
    let seekable = reader.is_seekable();
    let mime_type = reader.mime_type().map(str::to_owned);
    log::debug!("audio reader ready; byte_len={byte_len:?} seekable={seekable} mime={mime_type:?}");
    let buffered = BufReader::with_capacity(STREAM_BUFFER_BYTES, reader);

    let mut builder = Decoder::builder()
        .with_data(buffered)
        .with_seekable(seekable);
    if let Some(byte_len) = byte_len {
        builder = builder.with_byte_len(byte_len).with_seekable(seekable);
    }
    if let Some(mime_type) = mime_type.as_deref() {
        builder = builder.with_mime_type(mime_type);
    }

    let mut decoder = builder.build().map_err(|error| {
        log::error!(
            "audio decoder initialization failed; byte_len={byte_len:?} seekable={seekable} mime={mime_type:?}: {error}"
        );
        anyhow!("unsupported audio format: {error}")
    })?;
    let duration = decoder.total_duration().or(duration_hint);
    log::info!(
        "audio decoder ready; channels={} sample_rate={} duration_ms={:?}",
        decoder.channels().get(),
        decoder.sample_rate().get(),
        duration.map(|duration| duration.as_millis())
    );
    let start_position = duration
        .map(|duration| start_position.min(duration))
        .unwrap_or(start_position);
    if !start_position.is_zero() {
        decoder
            .try_seek(start_position)
            .map_err(|error| anyhow!("audio seek failed: {error}"))?;
    }
    let remaining_duration = duration.map(|duration| duration.saturating_sub(start_position));
    let (source, stream_error) = BufferedStreamSource::new(decoder, remaining_duration)?;
    Ok(PreparedStream {
        source,
        duration,
        error: stream_error,
        buffer_progress,
    })
}

struct PreparedStream {
    source: BufferedStreamSource,
    duration: Option<Duration>,
    error: SharedStreamError,
    buffer_progress: Arc<BufferProgress>,
}

enum StreamMessage {
    Samples(Vec<Sample>),
    End,
    Error(String),
}

struct BufferedStreamSource {
    receiver: Receiver<StreamMessage>,
    queued: VecDeque<Vec<Sample>>,
    current: std::vec::IntoIter<Sample>,
    channels: ChannelCount,
    sample_rate: SampleRate,
    duration: Option<Duration>,
    producer_finished: bool,
    terminal_error: Option<String>,
    shared_error: SharedStreamError,
    last_data: Instant,
    silence_remaining: usize,
}

impl BufferedStreamSource {
    fn new(
        decoder: Decoder<BufReader<HttpRangeReader>>,
        duration: Option<Duration>,
    ) -> Result<(Self, SharedStreamError)> {
        let channels = decoder.channels();
        let sample_rate = decoder.sample_rate();
        let (sender, receiver) = mpsc::sync_channel(DECODED_BUFFER_CHUNKS);
        let shared_error = Arc::new(Mutex::new(None));

        thread::Builder::new()
            .name("audio-decoder".to_string())
            .spawn(move || decode_stream(decoder, sender, duration, channels, sample_rate))
            .context("failed to start the audio decoder thread")?;

        let mut queued = VecDeque::new();
        let mut producer_finished = false;
        let mut terminal_error = None;
        let deadline = Instant::now() + STARTUP_BUFFER_TIMEOUT;

        while queued.len() < STARTUP_BUFFER_CHUNKS && !producer_finished {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            match receiver.recv_timeout(remaining) {
                Ok(StreamMessage::Samples(samples)) => queued.push_back(samples),
                Ok(StreamMessage::End) => producer_finished = true,
                Ok(StreamMessage::Error(error)) => {
                    terminal_error = Some(error);
                    producer_finished = true;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    terminal_error = Some("Audio decoder stopped unexpectedly".to_string());
                    producer_finished = true;
                }
            }
        }

        if queued.is_empty() {
            if let Some(error) = terminal_error {
                log::error!("audio decoder failed before startup buffer was ready: {error}");
                return Err(anyhow!(error));
            }
            log::error!(
                "audio startup buffer timed out; timeout_ms={}",
                STARTUP_BUFFER_TIMEOUT.as_millis()
            );
            return Err(anyhow!("timed out while buffering the audio stream"));
        }
        log::info!(
            "audio startup buffer ready; chunks={} target_chunks={} producer_finished={producer_finished}",
            queued.len(),
            STARTUP_BUFFER_CHUNKS
        );

        let source = Self {
            receiver,
            queued,
            current: Vec::new().into_iter(),
            channels,
            sample_rate,
            duration,
            producer_finished,
            terminal_error,
            shared_error: Arc::clone(&shared_error),
            last_data: Instant::now(),
            silence_remaining: 0,
        };
        Ok((source, shared_error))
    }

    fn finish(&mut self) -> Option<Sample> {
        if let Some(error) = self.terminal_error.take() {
            if let Ok(mut shared_error) = self.shared_error.lock() {
                *shared_error = Some(error);
            }
        }
        None
    }

    fn fail(&mut self, error: impl Into<String>) -> Option<Sample> {
        self.producer_finished = true;
        self.terminal_error = Some(error.into());
        self.finish()
    }
}

impl Iterator for BufferedStreamSource {
    type Item = Sample;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(sample) = self.current.next() {
                return Some(sample);
            }
            if let Some(samples) = self.queued.pop_front() {
                self.current = samples.into_iter();
                continue;
            }
            if self.producer_finished {
                return self.finish();
            }
            if self.silence_remaining > 0 {
                self.silence_remaining -= 1;
                return Some(0.0);
            }

            match self.receiver.try_recv() {
                Ok(StreamMessage::Samples(samples)) => {
                    self.last_data = Instant::now();
                    self.current = samples.into_iter();
                }
                Ok(StreamMessage::End) => self.producer_finished = true,
                Ok(StreamMessage::Error(error)) => {
                    self.terminal_error = Some(error);
                    self.producer_finished = true;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    if self.last_data.elapsed() >= STREAM_STALL_TIMEOUT {
                        return self.fail("Audio stream stalled for more than 30 seconds");
                    }
                    self.silence_remaining = usize::from(self.channels.get()).saturating_sub(1);
                    return Some(0.0);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    return self.fail("Audio decoder stopped unexpectedly");
                }
            }
        }
    }
}

impl Source for BufferedStreamSource {
    fn current_span_len(&self) -> Option<usize> {
        if self.terminal_error.is_none()
            && self.producer_finished
            && self.queued.is_empty()
            && self.current.len() == 0
        {
            Some(0)
        } else {
            None
        }
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.duration
    }
}

fn decode_stream(
    mut decoder: Decoder<BufReader<HttpRangeReader>>,
    sender: mpsc::SyncSender<StreamMessage>,
    duration: Option<Duration>,
    channels: ChannelCount,
    sample_rate: SampleRate,
) {
    let samples_per_chunk = DECODED_CHUNK_FRAMES * usize::from(channels.get());
    let mut decoded_samples = 0_u64;
    log::debug!(
        "audio decoder thread running; channels={} sample_rate={} chunk_samples={samples_per_chunk}",
        channels.get(),
        sample_rate.get()
    );

    loop {
        let samples: Vec<_> = decoder.by_ref().take(samples_per_chunk).collect();
        if samples.is_empty() {
            break;
        }
        decoded_samples = decoded_samples.saturating_add(samples.len() as u64);
        if sender.send(StreamMessage::Samples(samples)).is_err() {
            log::debug!("audio decoder stopped because playback source was dropped");
            return;
        }
    }

    log::debug!(
        "audio decoder reached stream end; decoded_samples={decoded_samples} decoded_ms={}",
        decoded_duration(decoded_samples, channels, sample_rate).as_millis()
    );
    let message = if stream_ended_too_early(decoded_samples, channels, sample_rate, duration) {
        let decoded_duration = decoded_duration(decoded_samples, channels, sample_rate);
        StreamMessage::Error(format!(
            "Audio stream ended unexpectedly at {}, expected about {}",
            format_playback(decoded_duration),
            format_playback(duration.unwrap_or_default())
        ))
    } else {
        StreamMessage::End
    };
    let _ = sender.send(message);
}

fn decoded_duration(
    decoded_samples: u64,
    channels: ChannelCount,
    sample_rate: SampleRate,
) -> Duration {
    let samples_per_second = u64::from(channels.get()) * u64::from(sample_rate.get());
    Duration::from_secs_f64(decoded_samples as f64 / samples_per_second as f64)
}

fn stream_ended_too_early(
    decoded_samples: u64,
    channels: ChannelCount,
    sample_rate: SampleRate,
    expected: Option<Duration>,
) -> bool {
    let Some(expected) = expected.filter(|duration| !duration.is_zero()) else {
        return false;
    };
    let actual = decoded_duration(decoded_samples, channels, sample_rate);
    let tolerance = Duration::from_secs_f64((expected.as_secs_f64() * 0.02).max(5.0));
    actual.saturating_add(tolerance) < expected
}

fn prepare_cache_dir(cache_dir: &Path) -> Result<()> {
    fs::create_dir_all(cache_dir).context("failed to create audio cache directory")?;

    let mut entries = fs::read_dir(cache_dir)
        .context("failed to read audio cache directory")?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            metadata.is_file().then(|| {
                (
                    entry.path(),
                    metadata.len(),
                    metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                )
            })
        })
        .collect::<Vec<_>>();
    let mut total = entries.iter().map(|(_, len, _)| len).sum::<u64>();
    if total <= MAX_AUDIO_CACHE_BYTES {
        return Ok(());
    }

    entries.sort_by_key(|(_, _, modified)| *modified);
    for (path, len, _) in entries {
        if total <= MAX_AUDIO_CACHE_BYTES {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
    Ok(())
}

fn cache_paths(cache_dir: &Path, cache_key: &str) -> (PathBuf, PathBuf) {
    let mut hasher = Md5::new();
    hasher.update(cache_key.as_bytes());
    let name = format!("{:x}", hasher.finalize());
    (
        cache_dir.join(format!("{name}.audio")),
        cache_dir.join(format!("{name}.part")),
    )
}

fn stream_endpoint(url: &str) -> String {
    Url::parse(url)
        .ok()
        .map(|url| {
            let host = url.host_str().unwrap_or("unknown");
            let port = url
                .port()
                .map(|port| format!(":{port}"))
                .unwrap_or_default();
            format!("{}://{host}{port}{}", url.scheme(), url.path())
        })
        .unwrap_or_else(|| "invalid-stream-url".to_string())
}

fn is_transcoded_stream(url: &str) -> bool {
    Url::parse(url).is_ok_and(|url| {
        url.query_pairs()
            .any(|(key, value)| key == "maxBitRate" && value != "0")
    })
}

struct BufferProgress {
    cached_bytes: AtomicU64,
    total_bytes: AtomicU64,
}

impl BufferProgress {
    fn new(cached_bytes: u64, total_bytes: Option<u64>) -> Self {
        Self {
            cached_bytes: AtomicU64::new(cached_bytes),
            total_bytes: AtomicU64::new(total_bytes.unwrap_or(0)),
        }
    }

    fn buffered_duration(&self, duration: Duration) -> Duration {
        let total = self.total_bytes.load(Ordering::Relaxed);
        if total == 0 {
            return Duration::ZERO;
        }
        let cached = self.cached_bytes.load(Ordering::Relaxed).min(total);
        duration.mul_f64(cached as f64 / total as f64)
    }
}

fn near_estimated_end(position: u64, estimated_len: u64) -> bool {
    estimated_len > 0 && position.saturating_mul(100) >= estimated_len.saturating_mul(95)
}

struct HttpRangeReader {
    client: Client,
    url: String,
    response: Option<Response>,
    response_position: u64,
    position: u64,
    len: Option<u64>,
    range_supported: bool,
    decoder_seekable: bool,
    length_is_estimate: bool,
    mime_type: Option<String>,
    cache_file: Option<File>,
    cached_len: u64,
    complete_path: PathBuf,
    partial_path: PathBuf,
    cache_complete: bool,
    logged_cache_bucket: u64,
    buffer_progress: Arc<BufferProgress>,
}

impl HttpRangeReader {
    fn open(client: Client, url: String, cache_dir: &Path, cache_key: &str) -> Result<Self> {
        let (complete_path, partial_path) = cache_paths(cache_dir, cache_key);
        if let Ok(file) = OpenOptions::new().read(true).open(&complete_path) {
            let len = file.metadata()?.len();
            log::info!(
                "audio cache hit; file={} bytes={len}",
                complete_path.display()
            );
            return Ok(Self {
                client,
                url,
                response: None,
                response_position: 0,
                position: 0,
                len: Some(len),
                range_supported: true,
                decoder_seekable: true,
                length_is_estimate: false,
                mime_type: None,
                cache_file: Some(file),
                cached_len: len,
                complete_path,
                partial_path,
                cache_complete: true,
                logged_cache_bucket: 10,
                buffer_progress: Arc::new(BufferProgress::new(len, Some(len))),
            });
        }

        let response = request_range(&client, &url, 0).context("failed to request audio stream")?;
        let range_supported = response.status() == StatusCode::PARTIAL_CONTENT;
        let len = content_range_len(&response).or_else(|| response.content_length());
        let length_is_estimate = is_transcoded_stream(&url);
        let decoder_seekable = range_supported && len.is_some() && !length_is_estimate;
        let mime_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let cache_file = fs::create_dir_all(cache_dir)
            .and_then(|()| {
                OpenOptions::new()
                    .create(true)
                    .read(true)
                    .write(true)
                    .truncate(false)
                    .open(&partial_path)
            })
            .map_err(|error| {
                log::warn!("audio cache is unavailable, continuing without it: {error}");
                error
            })
            .ok();
        let mut cached_len = cache_file
            .as_ref()
            .and_then(|file| file.metadata().ok())
            .map(|metadata| metadata.len())
            .unwrap_or(0);

        if !range_supported || len.is_some_and(|len| cached_len > len) {
            if cached_len > 0 {
                log::debug!(
                    "discarding partial audio cache; cached_bytes={cached_len} range_supported={range_supported} total_bytes={len:?}"
                );
            }
            if let Some(file) = cache_file.as_ref() {
                if let Err(error) = file.set_len(0) {
                    log::warn!("failed to reset partial audio cache: {error}");
                }
            }
            cached_len = 0;
        }

        log::info!(
            "audio network stream opened; endpoint={} status={} bytes={len:?} range_supported={range_supported} decoder_seekable={decoder_seekable} mime={mime_type:?} cached_bytes={cached_len}",
            stream_endpoint(&url),
            response.status()
        );

        Ok(Self {
            client,
            url,
            response: Some(response),
            response_position: 0,
            position: 0,
            len,
            range_supported,
            decoder_seekable,
            length_is_estimate,
            mime_type,
            cache_file,
            cached_len,
            complete_path,
            partial_path,
            cache_complete: false,
            logged_cache_bucket: len
                .filter(|len| *len > 0)
                .map(|len| cached_len.saturating_mul(10) / len)
                .unwrap_or(0),
            buffer_progress: Arc::new(BufferProgress::new(cached_len, len)),
        })
    }

    fn len(&self) -> Option<u64> {
        self.len
    }

    fn is_seekable(&self) -> bool {
        self.cache_complete || self.decoder_seekable
    }

    fn mime_type(&self) -> Option<&str> {
        self.mime_type.as_deref()
    }

    fn reopen_at(&mut self, position: u64) -> io::Result<()> {
        if position != 0 && !self.range_supported {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "audio server does not support HTTP range requests",
            ));
        }

        log::debug!(
            "reopening audio HTTP stream; endpoint={} byte_position={position}",
            stream_endpoint(&self.url)
        );
        let response = request_range(&self.client, &self.url, position).map_err(io_error)?;
        if position != 0 && response.status() != StatusCode::PARTIAL_CONTENT {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "audio server ignored the requested byte range",
            ));
        }
        self.range_supported |= response.status() == StatusCode::PARTIAL_CONTENT;
        self.response = Some(response);
        self.response_position = position;
        Ok(())
    }

    fn read_cached(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let available = self.cached_len.saturating_sub(self.position);
        let read_len = usize::try_from(available.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let file = self
            .cache_file
            .as_mut()
            .ok_or_else(|| io::Error::other("audio cache file is closed"))?;
        file.seek(SeekFrom::Start(self.position))?;
        let read = file.read(&mut buffer[..read_len])?;
        self.position = self.position.saturating_add(read as u64);
        Ok(read)
    }

    fn finish_estimated_stream(&mut self) {
        let estimated_len = self.len;
        self.len = Some(self.position);
        self.buffer_progress
            .total_bytes
            .store(self.position, Ordering::Relaxed);
        self.logged_cache_bucket = 10;
        log::info!(
            "transcoded stream reached actual end; actual_bytes={} estimated_bytes={estimated_len:?}",
            self.position
        );
        log::info!(
            "audio cache progress; cached_bytes={} total_bytes={} percent=100",
            self.cached_len,
            self.position
        );
    }

    fn near_estimated_end(&self) -> bool {
        self.length_is_estimate
            && self
                .len
                .is_some_and(|len| near_estimated_end(self.position, len))
    }

    fn cache_network_bytes(&mut self, start: u64, bytes: &[u8]) -> io::Result<()> {
        if start != self.cached_len || bytes.is_empty() {
            return Ok(());
        }
        let Some(file) = self.cache_file.as_mut() else {
            return Ok(());
        };
        if let Err(error) = file
            .seek(SeekFrom::Start(start))
            .and_then(|_| file.write_all(bytes))
        {
            log::warn!("failed to write audio cache, continuing without it: {error}");
            self.cache_file = None;
            self.cached_len = 0;
            self.buffer_progress
                .cached_bytes
                .store(0, Ordering::Relaxed);
            return Ok(());
        }
        self.cached_len = self.cached_len.saturating_add(bytes.len() as u64);
        self.buffer_progress
            .cached_bytes
            .store(self.cached_len, Ordering::Relaxed);
        if let Some(total) = self.len.filter(|total| *total > 0) {
            let bucket = (self.cached_len.saturating_mul(10) / total).min(10);
            if bucket > self.logged_cache_bucket {
                self.logged_cache_bucket = bucket;
                log::info!(
                    "audio cache progress; cached_bytes={} total_bytes={total} percent={}",
                    self.cached_len,
                    bucket * 10
                );
            }
        }
        Ok(())
    }
}

impl Read for HttpRangeReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.position < self.cached_len {
            let position = self.position;
            match self.read_cached(buffer) {
                Ok(read) => return Ok(read),
                Err(error) if !self.cache_complete => {
                    log::warn!(
                        "failed to read partial audio cache, continuing without it: {error}"
                    );
                    self.cache_file = None;
                    self.cached_len = 0;
                    self.buffer_progress
                        .cached_bytes
                        .store(0, Ordering::Relaxed);
                    self.position = position;
                }
                Err(error) => return Err(error),
            }
        }
        if self.cache_complete || self.len == Some(self.position) {
            return Ok(0);
        }
        if self.response_position != self.position {
            self.reopen_at(self.position)?;
        }

        for attempt in 0..=HTTP_READ_RETRIES {
            let start = self.position;
            let result = self
                .response
                .as_mut()
                .ok_or_else(|| io::Error::other("audio HTTP response is unavailable"))?
                .read(buffer);
            match result {
                Ok(0) if self.near_estimated_end() => {
                    self.finish_estimated_stream();
                    return Ok(0);
                }
                Ok(0) if self.len.is_some_and(|len| self.position < len) => {
                    if !self.range_supported || attempt == HTTP_READ_RETRIES {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "audio stream ended before the advertised content length",
                        ));
                    }
                    thread::sleep(Duration::from_millis(250 * (attempt as u64 + 1)));
                    self.reopen_at(self.position)?;
                }
                Ok(read) => {
                    self.cache_network_bytes(start, &buffer[..read])?;
                    self.position = self.position.saturating_add(read as u64);
                    self.response_position = self.position;
                    return Ok(read);
                }
                Err(error) => {
                    if self.near_estimated_end() && attempt >= TRANSCODE_END_RETRIES {
                        self.finish_estimated_stream();
                        return Ok(0);
                    }
                    if !self.range_supported || attempt == HTTP_READ_RETRIES {
                        return Err(error);
                    }
                    if self.near_estimated_end() {
                        log::debug!(
                            "transcoded stream read ended near estimated length; byte={} retry={}: {error}",
                            self.position,
                            attempt + 1
                        );
                    } else {
                        log::warn!(
                            "audio stream read failed at byte {}, retrying: {error}",
                            self.position
                        );
                    }
                    thread::sleep(Duration::from_millis(250 * (attempt as u64 + 1)));
                    self.reopen_at(self.position)?;
                }
            }
        }
        unreachable!("audio stream retry loop always returns")
    }
}

impl Seek for HttpRangeReader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.position = seek_target(self.position, self.len, from)?;
        Ok(self.position)
    }
}

impl Drop for HttpRangeReader {
    fn drop(&mut self) {
        let fully_cached = self
            .len
            .is_some_and(|len| len > 0 && self.cached_len >= len);
        if self.cache_complete || !fully_cached {
            return;
        }
        self.cache_file.take();
        if let Err(error) = fs::rename(&self.partial_path, &self.complete_path) {
            log::warn!("failed to finalize audio cache: {error}");
        } else {
            log::info!(
                "audio cache finalized; file={} bytes={}",
                self.complete_path.display(),
                self.cached_len
            );
        }
    }
}

fn request_range(client: &Client, url: &str, position: u64) -> reqwest::Result<Response> {
    let response = client
        .get(url)
        .header(RANGE, format!("bytes={position}-"))
        .send()?;
    log::debug!(
        "audio HTTP response; endpoint={} range_start={position} status={} content_length={:?} content_range={:?} content_type={:?}",
        stream_endpoint(url),
        response.status(),
        response.content_length(),
        response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok()),
        response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
    );
    response.error_for_status()
}

fn content_range_len(response: &Response) -> Option<u64> {
    response
        .headers()
        .get(CONTENT_RANGE)?
        .to_str()
        .ok()?
        .rsplit_once('/')?
        .1
        .parse()
        .ok()
}

fn seek_target(current: u64, len: Option<u64>, from: SeekFrom) -> io::Result<u64> {
    let target = match from {
        SeekFrom::Start(position) => i128::from(position),
        SeekFrom::Current(offset) => i128::from(current) + i128::from(offset),
        SeekFrom::End(offset) => {
            let len = len.ok_or_else(|| {
                io::Error::new(io::ErrorKind::Unsupported, "audio length is unknown")
            })?;
            i128::from(len) + i128::from(offset)
        }
    };
    if target < 0 || target > i128::from(u64::MAX) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid audio stream seek",
        ));
    }
    let target = target as u64;
    if len.is_some_and(|len| target > len) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "audio stream seek is past the end",
        ));
    }
    Ok(target)
}

fn io_error(error: reqwest::Error) -> io::Error {
    io::Error::other(error)
}

fn set_state(state: &Arc<Mutex<PlaybackState>>, update: impl FnOnce(&mut PlaybackState)) {
    if let Ok(mut state) = state.lock() {
        update(&mut state);
    }
}

pub fn format_playback(duration: Duration) -> String {
    let total = duration.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
