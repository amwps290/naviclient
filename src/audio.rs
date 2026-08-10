use std::fs::File;
use std::io::{BufReader, Seek, SeekFrom};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use rodio::{Decoder, DeviceSinkBuilder, Player, Source};

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
    Play { url: String },
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

    pub fn play(&self, url: String) {
        let _ = self.tx.send(Command::Play { url });
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
                Command::Play { url } => {
                    set_state(&state, |s| {
                        s.active = true;
                        s.paused = false;
                        s.ended = false;
                        s.position = Duration::ZERO;
                        s.duration = None;
                    });
                    match play_url(&url, volume) {
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
                            set_state(&state, |s| {
                                s.active = false;
                                s.ended = true;
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
                    let _ = player.try_seek(position);
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

fn play_url(url: &str, _volume: f32) -> Result<(Decoder<BufReader<File>>, Option<Duration>)> {
    let client = reqwest::blocking::Client::builder()
        .timeout(None)
        .build()
        .context("failed to build streaming client")?;

    let mut response = client
        .get(url)
        .send()
        .context("failed to request audio stream")?
        .error_for_status()
        .context("audio stream request failed")?;

    let mut file = tempfile::tempfile().context("failed to create temporary audio file")?;
    std::io::copy(&mut response, &mut file).context("failed to download audio stream")?;
    file.seek(SeekFrom::Start(0))?;

    let decoder = Decoder::new(BufReader::new(file))
        .map_err(|error| anyhow!("unsupported audio format: {error}"))?;
    let duration = decoder.total_duration();
    Ok((decoder, duration))
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
