use std::collections::VecDeque;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use reqwest::blocking::{Client, Response};
use reqwest::header::{CONTENT_RANGE, CONTENT_TYPE, RANGE};
use reqwest::StatusCode;
use rodio::{
    cpal::BufferSize, ChannelCount, Decoder, DeviceSinkBuilder, Player, Sample, SampleRate, Source,
};

const STREAM_BUFFER_BYTES: usize = 512 * 1024;
const DECODED_CHUNK_FRAMES: usize = 4096;
const DECODED_BUFFER_CHUNKS: usize = 16;
const STARTUP_BUFFER_CHUNKS: usize = 4;
const STARTUP_BUFFER_TIMEOUT: Duration = Duration::from_secs(12);
const STREAM_STALL_TIMEOUT: Duration = Duration::from_secs(15);
const OUTPUT_BUFFER_FRAMES: u32 = 4096;
const HTTP_READ_RETRIES: usize = 3;

type SharedStreamError = Arc<Mutex<Option<String>>>;

#[derive(Clone, Debug, Default)]
pub struct PlaybackState {
    pub active: bool,
    pub paused: bool,
    pub ended: bool,
    pub position: Duration,
    pub duration: Option<Duration>,
    pub error: Option<String>,
}

#[derive(Debug)]
enum Command {
    Play {
        url: String,
        duration_hint: Option<Duration>,
    },
    Pause,
    Resume,
    Stop,
    SetVolume(f32),
    Seek(Duration),
}

#[derive(Clone)]
pub struct AudioHandle {
    tx: Sender<Command>,
    state: Arc<Mutex<PlaybackState>>,
}

impl AudioHandle {
    pub fn start() -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(PlaybackState::default()));
        let worker_state = Arc::clone(&state);
        thread::Builder::new()
            .name("audio-worker".to_string())
            .spawn(move || run_worker(rx, worker_state))
            .context("failed to start audio worker")?;
        Ok(Self { tx, state })
    }

    pub fn play(&self, url: String, duration_hint: Option<Duration>) {
        let _ = self.tx.send(Command::Play { url, duration_hint });
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

fn run_worker(rx: Receiver<Command>, state: Arc<Mutex<PlaybackState>>) {
    let Ok(mut device_sink) = open_audio_sink() else {
        set_state(&state, |s| {
            s.active = false;
            s.ended = false;
            s.error = Some("Failed to open the default audio output device".to_string());
        });
        return;
    };
    let Ok(client) = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(None)
        .build()
    else {
        set_state(&state, |s| {
            s.active = false;
            s.ended = false;
            s.error = Some("Failed to create the audio HTTP client".to_string());
        });
        return;
    };

    device_sink.log_on_drop(false);
    let mixer = device_sink.mixer().clone();
    let (player, source) = Player::new();
    mixer.add(source);

    let mut volume = 1.0;
    let mut duration: Option<Duration> = None;
    let mut stream_error: Option<SharedStreamError> = None;
    let mut startup_error: Option<String> = None;
    let mut current_url: Option<String> = None;
    let mut position_offset = Duration::ZERO;

    loop {
        let command = match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(command) => Some(command),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };

        if let Some(command) = command {
            match command {
                Command::Play { url, duration_hint } => {
                    player.stop();
                    player.clear();
                    startup_error = None;
                    current_url = Some(url.clone());
                    position_offset = Duration::ZERO;
                    set_state(&state, |s| {
                        s.active = true;
                        s.paused = false;
                        s.ended = false;
                        s.position = Duration::ZERO;
                        s.duration = duration_hint;
                        s.error = None;
                    });
                    match play_url(client.clone(), &url, duration_hint, Duration::ZERO) {
                        Ok(stream) => {
                            player.set_volume(volume);
                            player.append(stream.source);
                            player.play();
                            duration = stream.duration;
                            stream_error = Some(stream.error);
                            set_state(&state, |s| {
                                s.active = true;
                                s.paused = false;
                                s.ended = false;
                                s.position = Duration::ZERO;
                                s.duration = duration;
                                s.error = None;
                            });
                        }
                        Err(error) => {
                            duration = None;
                            stream_error = None;
                            startup_error = Some(format!("Unable to start playback: {error:#}"));
                            set_state(&state, |s| {
                                s.active = false;
                                s.ended = false;
                                s.duration = None;
                                s.error = startup_error.clone();
                            });
                            log::warn!("playback failed: {error:#}");
                        }
                    }
                }
                Command::Pause => {
                    player.pause();
                    set_state(&state, |s| s.paused = true);
                }
                Command::Resume => {
                    player.play();
                    set_state(&state, |s| s.paused = false);
                }
                Command::Stop => {
                    player.stop();
                    player.clear();
                    duration = None;
                    stream_error = None;
                    startup_error = None;
                    current_url = None;
                    position_offset = Duration::ZERO;
                    set_state(&state, |s| {
                        s.active = false;
                        s.paused = false;
                        s.ended = true;
                        s.position = Duration::ZERO;
                        s.duration = None;
                        s.error = None;
                    });
                }
                Command::SetVolume(new_volume) => {
                    volume = new_volume.clamp(0.0, 1.0);
                    player.set_volume(volume);
                }
                Command::Seek(position) => {
                    let Some(url) = current_url.clone() else {
                        continue;
                    };
                    let target = duration
                        .map(|duration| position.min(duration))
                        .unwrap_or(position);
                    let was_paused = player.is_paused();
                    player.stop();
                    player.clear();
                    startup_error = None;
                    position_offset = target;
                    set_state(&state, |s| {
                        s.active = true;
                        s.ended = false;
                        s.position = target;
                        s.error = None;
                    });

                    match play_url(client.clone(), &url, duration, target) {
                        Ok(stream) => {
                            player.set_volume(volume);
                            player.append(stream.source);
                            if was_paused {
                                player.pause();
                            } else {
                                player.play();
                            }
                            duration = stream.duration;
                            stream_error = Some(stream.error);
                        }
                        Err(error) => {
                            stream_error = None;
                            startup_error = Some(format!("Unable to seek audio stream: {error:#}"));
                            log::warn!("playback seek failed: {error:#}");
                        }
                    }
                }
            }
        }

        let position = position_offset.saturating_add(player.get_pos());
        let paused = player.is_paused();
        let empty = player.empty();
        let error = if empty {
            startup_error.clone().or_else(|| {
                stream_error
                    .as_ref()
                    .and_then(|error| error.lock().ok()?.clone())
            })
        } else {
            None
        };
        let ended = empty && error.is_none();
        set_state(&state, |s| {
            s.position = position;
            s.duration = duration;
            s.paused = paused;
            s.ended = ended;
            s.active = !empty;
            s.error = error;
        });
    }
}

fn open_audio_sink() -> Result<rodio::MixerDeviceSink, rodio::DeviceSinkError> {
    DeviceSinkBuilder::from_default_device()
        .and_then(|builder| {
            builder
                .with_buffer_size(BufferSize::Fixed(OUTPUT_BUFFER_FRAMES))
                .open_sink_or_fallback()
        })
        .or_else(|_| DeviceSinkBuilder::open_default_sink())
}

fn play_url(
    client: Client,
    url: &str,
    duration_hint: Option<Duration>,
    start_position: Duration,
) -> Result<PreparedStream> {
    let reader = HttpRangeReader::open(client, url.to_string())?;
    let byte_len = reader.len();
    let seekable = reader.is_seekable();
    let mime_type = reader.mime_type().map(str::to_owned);
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

    let mut decoder = builder
        .build()
        .map_err(|error| anyhow!("unsupported audio format: {error}"))?;
    let duration = decoder.total_duration().or(duration_hint);
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
    })
}

struct PreparedStream {
    source: BufferedStreamSource,
    duration: Option<Duration>,
    error: SharedStreamError,
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
                return Err(anyhow!(error));
            }
            return Err(anyhow!("timed out while buffering the audio stream"));
        }

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
                        return self.fail("Audio stream stalled for more than 15 seconds");
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

    loop {
        let samples: Vec<_> = decoder.by_ref().take(samples_per_chunk).collect();
        if samples.is_empty() {
            break;
        }
        decoded_samples = decoded_samples.saturating_add(samples.len() as u64);
        if sender.send(StreamMessage::Samples(samples)).is_err() {
            return;
        }
    }

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

struct HttpRangeReader {
    client: Client,
    url: String,
    response: Response,
    position: u64,
    len: Option<u64>,
    range_supported: bool,
    mime_type: Option<String>,
}

impl HttpRangeReader {
    fn open(client: Client, url: String) -> Result<Self> {
        let response = request_range(&client, &url, 0).context("failed to request audio stream")?;
        let range_supported = response.status() == StatusCode::PARTIAL_CONTENT;
        let len = content_range_len(&response).or_else(|| response.content_length());
        let mime_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);

        Ok(Self {
            client,
            url,
            response,
            position: 0,
            len,
            range_supported,
            mime_type,
        })
    }

    fn len(&self) -> Option<u64> {
        self.len
    }

    fn is_seekable(&self) -> bool {
        self.range_supported && self.len.is_some()
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

        let response = request_range(&self.client, &self.url, position).map_err(io_error)?;
        if position != 0 && response.status() != StatusCode::PARTIAL_CONTENT {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "audio server ignored the requested byte range",
            ));
        }
        self.range_supported |= response.status() == StatusCode::PARTIAL_CONTENT;
        self.response = response;
        self.position = position;
        Ok(())
    }
}

impl Read for HttpRangeReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        for attempt in 0..=HTTP_READ_RETRIES {
            match self.response.read(buffer) {
                Ok(0) if self.len.is_some_and(|len| self.position < len) => {
                    if !self.range_supported || attempt == HTTP_READ_RETRIES {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "audio stream ended before the advertised content length",
                        ));
                    }
                    thread::sleep(Duration::from_millis(150 * (attempt as u64 + 1)));
                    self.reopen_at(self.position)?;
                }
                Ok(read) => {
                    self.position = self.position.saturating_add(read as u64);
                    return Ok(read);
                }
                Err(error) => {
                    if !self.range_supported || attempt == HTTP_READ_RETRIES {
                        return Err(error);
                    }
                    log::warn!(
                        "audio stream read failed at byte {}, retrying: {error}",
                        self.position
                    );
                    thread::sleep(Duration::from_millis(150 * (attempt as u64 + 1)));
                    self.reopen_at(self.position)?;
                }
            }
        }
        unreachable!("audio stream retry loop always returns")
    }
}

impl Seek for HttpRangeReader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let target = seek_target(self.position, self.len, from)?;
        if target != self.position {
            self.reopen_at(target)?;
        }
        Ok(self.position)
    }
}

fn request_range(client: &Client, url: &str, position: u64) -> reqwest::Result<Response> {
    client
        .get(url)
        .header(RANGE, format!("bytes={position}-"))
        .send()?
        .error_for_status()
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
