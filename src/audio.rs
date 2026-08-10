use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use reqwest::blocking::{Client, Response};
use reqwest::header::{CONTENT_RANGE, CONTENT_TYPE, RANGE};
use reqwest::StatusCode;
use rodio::{Decoder, DeviceSinkBuilder, Player, Source};

const STREAM_BUFFER_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Default)]
pub struct PlaybackState {
    pub active: bool,
    pub paused: bool,
    pub ended: bool,
    pub position: Duration,
    pub duration: Option<Duration>,
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
    let Ok(mut device_sink) = DeviceSinkBuilder::open_default_sink() else {
        set_state(&state, |s| {
            s.active = false;
            s.ended = true;
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
            s.ended = true;
        });
        return;
    };

    device_sink.log_on_drop(false);
    let mixer = device_sink.mixer().clone();
    let (player, source) = Player::new();
    mixer.add(source);

    let mut volume = 1.0;
    let mut duration: Option<Duration> = None;

    loop {
        let command = match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(command) => Some(command),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };

        if let Some(command) = command {
            match command {
                Command::Play { url, duration_hint } => {
                    set_state(&state, |s| {
                        s.active = true;
                        s.paused = false;
                        s.ended = false;
                        s.position = Duration::ZERO;
                        s.duration = duration_hint;
                    });
                    match play_url(client.clone(), &url, duration_hint) {
                        Ok((decoder, new_duration)) => {
                            player.stop();
                            player.clear();
                            player.set_volume(volume);
                            player.append(decoder);
                            player.play();
                            duration = new_duration;
                            set_state(&state, |s| {
                                s.active = true;
                                s.paused = false;
                                s.ended = false;
                                s.position = Duration::ZERO;
                                s.duration = duration;
                            });
                        }
                        Err(error) => {
                            duration = None;
                            set_state(&state, |s| {
                                s.active = false;
                                s.ended = true;
                                s.duration = None;
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
                    set_state(&state, |s| {
                        s.active = false;
                        s.paused = false;
                        s.ended = true;
                        s.position = Duration::ZERO;
                        s.duration = None;
                    });
                }
                Command::SetVolume(new_volume) => {
                    volume = new_volume.clamp(0.0, 1.0);
                    player.set_volume(volume);
                }
                Command::Seek(position) => {
                    if let Err(error) = player.try_seek(position) {
                        log::warn!("playback seek failed: {error}");
                    }
                }
            }
        }

        let position = player.get_pos();
        let paused = player.is_paused();
        let ended = player.empty();
        set_state(&state, |s| {
            s.position = position;
            s.duration = duration;
            s.paused = paused;
            s.ended = ended;
            s.active = !ended;
        });
    }
}

fn play_url(
    client: Client,
    url: &str,
    duration_hint: Option<Duration>,
) -> Result<(Decoder<BufReader<HttpRangeReader>>, Option<Duration>)> {
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

    let decoder = builder
        .build()
        .map_err(|error| anyhow!("unsupported audio format: {error}"))?;
    let duration = decoder.total_duration().or(duration_hint);
    Ok((decoder, duration))
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
        let read = self.response.read(buffer)?;
        self.position = self.position.saturating_add(read as u64);
        Ok(read)
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
    io::Error::new(io::ErrorKind::Other, error)
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
}
