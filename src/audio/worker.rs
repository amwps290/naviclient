//! Audio worker startup, loop-owned resources, and ordered playback phases.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::blocking::Client;
use rust_i18n::t;

use super::device_output::{AudioOutput, DevicePlayback};
use super::next_track::NextTrack;
use super::output::{open_audio_device, report_output_device};
use super::output_track::OutputTrack;
use super::preparation::Preparation;
use super::preparation_state::PreparationToken;
use super::stream_cache::prepare_cache_dir;
use super::track_state::TrackState;
use super::track_transition::Transition;
use super::transport::{self, WorkerCommand};
use super::{set_state, PlaybackState};
use crate::models::EqSettings;

const HTTP_IO_TIMEOUT: Duration = Duration::from_secs(12);

pub(super) mod commands;
pub(super) mod playback;

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

#[cfg(test)]
mod control_tests;
#[cfg(test)]
mod tests;

pub(super) fn run_worker(
    rx: Receiver<WorkerCommand>,
    state: Arc<Mutex<PlaybackState>>,
    initial_cache_dir: PathBuf,
    output_device: Option<String>,
) {
    let _guard = transport::WorkerGuard(state.clone());
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

    let output = AudioOutput::new(DevicePlayback::new(device_sink, output_name));
    run_playback(rx, state, initial_cache_dir, client, output);
}

pub(super) fn run_playback(
    rx: Receiver<WorkerCommand>,
    state: Arc<Mutex<PlaybackState>>,
    initial_cache_dir: PathBuf,
    client: Client,
    output: AudioOutput,
) {
    PlaybackWorker::new(rx, state, initial_cache_dir, client, output).run();
}

type ReadyOutput = (PreparationToken, OutputTrack);

struct PlaybackWorker {
    // Match the loop's previous teardown order: cancel foreground and next-track
    // work, and drop current stream handles, before releasing the output device.
    preparation: Preparation,
    pending_commands: VecDeque<WorkerCommand>,
    cache_dir: PathBuf,
    current: TrackState,
    next_track: NextTrack,
    output: AudioOutput,
    client: Client,
    state: Arc<Mutex<PlaybackState>>,
    rx: Receiver<WorkerCommand>,
    volume: f32,
    command_serial: u64,
}

impl PlaybackWorker {
    fn new(
        rx: Receiver<WorkerCommand>,
        state: Arc<Mutex<PlaybackState>>,
        initial_cache_dir: PathBuf,
        client: Client,
        output: AudioOutput,
    ) -> Self {
        let next_track = NextTrack::new(client.clone(), output.handoff.clone());

        let volume = 1.0;
        let current = TrackState::default();
        let cache_dir = initial_cache_dir;
        if let Err(error) = prepare_cache_dir(&cache_dir) {
            log::warn!("audio cache is unavailable: {error:#}");
        } else {
            log::debug!("audio cache ready; directory={}", cache_dir.display());
        }

        let pending_commands = VecDeque::new();
        let preparation = Preparation::new();
        let command_serial = 0;

        Self {
            preparation,
            pending_commands,
            cache_dir,
            current,
            next_track,
            output,
            client,
            state,
            rx,
            volume,
            command_serial,
        }
    }

    fn run(mut self) {
        loop {
            let command = if let Some(command) = self.pending_commands.pop_front() {
                Some(command)
            } else {
                match self.rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(command) => Some(command),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            };

            if let Some((next, event)) = self.next_track.take_started() {
                self.current.accept_gapless(next, event);
                log::info!("gapless track started; ticket={}", event.id);
            }

            let mut ready_output = if let Some(command) = command {
                self.handle_command(command)
            } else {
                None
            };
            self.poll_preparation(&mut ready_output);
            if let Some(ready) = ready_output {
                self.attach_output(ready);
            }
            self.poll_next_and_publish();
        }
    }
}
