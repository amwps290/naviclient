use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::models::EqSettings;
use crate::source::HttpResource;

mod buffered_stream;
mod decoder;
mod device_output;
mod eq;
mod error;
mod gapless;
mod http_stream;
mod loudness_meter;
mod next_track;
mod output;
mod output_track;
mod preload;
mod preparation;
mod preparation_state;
pub(crate) mod probe;
mod stream_cache;
mod stream_control;
mod stream_response;
#[cfg(test)]
pub(crate) mod test_transport;
mod track_state;
mod track_transition;
mod transport;
mod worker;

pub use error::StreamFailure;
pub use output::{list_output_devices, OutputDevice};
pub use track_transition::TrackTransition;

use track_transition::Transition;
use transport::{Transport, WorkerCommand};
use worker::{run_worker, Command};

/// 一条曲目在一次播放中最多自动重连几次（用尽后向用户报错）。
pub(crate) const PLAYBACK_RECONNECT_ATTEMPTS: u8 = 2;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlaybackState {
    /// Last transport command whose state the worker has published.
    pub(crate) command_serial: u64,
    /// The worker exited, including output/client initialization failures.
    pub(crate) worker_closed: bool,
    /// The current track is attached to the output, beyond startup/seek preparation.
    pub(crate) output_connected: bool,
    pub active: bool,
    pub paused: bool,
    pub ended: bool,
    pub position: Duration,
    pub buffered: Duration,
    /// The remaining audio is decoded locally; no further network reads needed.
    pub fully_buffered: bool,
    pub transition_id: Option<u64>,
    pub duration: Option<Duration>,
    pub error: Option<String>,
    /// 最近一次失败的结构化原因；与 `error` 同时出现。
    pub failure: Option<StreamFailure>,
    /// 播放中音频数据停止流入的时长；`None` 表示数据正常到达。
    pub stall: Option<Duration>,
    /// Incremented after a seek completes, for system media-session notifications.
    pub seek_serial: u64,
    /// 当前歌曲已测响度（RMS，dB），未测时为 0.0。
    pub loudness_db: f32,
    /// 当前歌曲已测峰值（0..=1），未测时为 0.0。
    pub loudness_peak: f32,
    /// 当前音频输出设备名；None 表示跟随系统默认。
    pub output_name: Option<String>,
    /// 输出设备实际采样率（诊断展示用）。
    pub output_sample_rate: u32,
    /// 输出设备实际声道数（诊断展示用）。
    pub output_channels: u16,
}

#[derive(Clone)]
pub struct AudioHandle {
    tx: Sender<WorkerCommand>,
    state: Arc<Mutex<PlaybackState>>,
    transition: Arc<Mutex<Option<Arc<Transition>>>>,
    transport: Arc<Mutex<Transport>>,
}

/// Initial transport intent travels with the selected resource in one command.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlaybackStart {
    pub position: Duration,
    pub paused: bool,
    pub preload_id: Option<u64>,
}

impl AudioHandle {
    pub fn start(cache_dir: PathBuf, output_device: Option<String>) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let state = Arc::new(Mutex::new(PlaybackState::default()));
        let worker_state = Arc::clone(&state);
        let transition = Arc::new(Mutex::new(None));
        thread::Builder::new()
            .name("audio-worker".to_string())
            .spawn(move || run_worker(rx, worker_state, cache_dir, output_device))
            .context("failed to start audio worker")?;
        log::debug!("audio worker thread started");
        Ok(Self {
            tx,
            state,
            transition,
            transport: Arc::new(Mutex::new(Transport::default())),
        })
    }

    pub fn play(
        &self,
        url: HttpResource,
        cache_key: String,
        duration_hint: Option<Duration>,
        gain: f32,
        start: PlaybackStart,
    ) -> bool {
        self.cancel_transition();
        self.transition.lock().unwrap().take();
        if self
            .send(Command::Play {
                url,
                cache_key,
                duration_hint,
                start,
                gain,
            })
            .is_err()
        {
            log::error!("failed to send play command: audio worker is unavailable");
            return false;
        }
        true
    }

    pub fn pause(&self) {
        let _ = self.send(Command::Pause);
    }

    /// Whether the worker accepted the reservation, before preparation starts.
    pub fn preload(
        &self,
        id: u64,
        url: HttpResource,
        cache_key: String,
        duration_hint: Option<Duration>,
        gain: f32,
    ) -> bool {
        let transition = Transition::new(id, duration_hint);
        *self.transition.lock().unwrap() = Some(transition.clone());
        if self
            .send(Command::Preload {
                id,
                url,
                cache_key,
                duration_hint,
                gain,
                transition: transition.clone(),
            })
            .is_err()
        {
            transition.cancel();
            log::error!("failed to send preload command: audio worker is unavailable");
            return false;
        }
        true
    }

    pub fn cancel_preload(&self) -> Option<TrackTransition> {
        let event = self.freeze_preload();
        let _ = self.send(Command::CancelPreload);
        event
    }

    pub fn freeze_preload(&self) -> Option<TrackTransition> {
        self.cancel_transition();
        self.take_transition()
    }

    fn cancel_transition(&self) {
        if let Some(transition) = self.transition.lock().unwrap().as_ref() {
            transition.cancel();
        }
    }

    pub fn take_transition(&self) -> Option<TrackTransition> {
        self.transition.lock().unwrap().as_ref()?.take_event()
    }

    pub fn resume(&self) {
        let _ = self.send(Command::Resume);
    }

    pub fn stop(&self) {
        self.cancel_transition();
        self.transition.lock().unwrap().take();
        let _ = self.send(Command::Stop);
    }

    pub fn set_volume(&self, volume: f32) {
        let _ = self.send(Command::SetVolume(volume));
    }

    pub fn set_cache_directory(&self, cache_dir: PathBuf) {
        self.cancel_transition();
        if self.send(Command::SetCacheDirectory(cache_dir)).is_err() {
            log::error!("failed to update cache directory: audio worker is unavailable");
        }
    }

    /// 切换输出设备；播放中会在新设备上从当前位置继续，暂停意图保留。
    pub fn set_output_device(&self, name: Option<String>) {
        self.cancel_transition();
        if self.send(Command::SetOutputDevice(name)).is_err() {
            log::error!("failed to switch output device: audio worker is unavailable");
        }
    }

    /// 更新均衡器设置；音频线程内重算系数，正在播放的曲目在下一个
    /// 版本检查点（约千帧）生效。
    pub fn set_equalizer(&self, settings: EqSettings) {
        if self.send(Command::SetEqualizer(settings)).is_err() {
            log::error!("failed to update equalizer: audio worker is unavailable");
        }
    }

    pub fn seek(&self, position: Duration) {
        self.cancel_transition();
        let _ = self.send(Command::Seek(position));
    }

    /// Responsive control state, including commands accepted but not yet processed.
    pub fn state(&self) -> PlaybackState {
        self.transport
            .lock()
            .unwrap()
            .observe(self.observed_state())
    }

    /// Actual output progress for listening/reporting. Pending seek targets are
    /// inactive baselines, and replacement tracks cannot inherit old activity.
    pub(crate) fn progress(&self) -> PlaybackState {
        self.transport
            .lock()
            .unwrap()
            .progress(self.observed_state())
    }

    fn send(&self, command: Command) -> std::result::Result<(), mpsc::SendError<WorkerCommand>> {
        self.transport
            .lock()
            .unwrap()
            .send(&self.tx, self.observed_state(), command)
    }

    fn observed_state(&self) -> PlaybackState {
        let mut state = self
            .state
            .lock()
            .map(|state| state.clone())
            .unwrap_or_default();
        // The output callback can lead the worker by one poll. Never expose the
        // previous song's end/position/loudness as the newly started song.
        if let Some(transition) = self.transition.lock().unwrap().as_ref() {
            if !state.worker_closed
                && transition.event().is_some()
                && state.transition_id != Some(transition.id)
            {
                state = PlaybackState {
                    active: true,
                    output_connected: true,
                    paused: state.paused,
                    duration: transition.duration,
                    transition_id: Some(transition.id),
                    seek_serial: state.seek_serial,
                    command_serial: state.command_serial,
                    output_name: state.output_name,
                    output_sample_rate: state.output_sample_rate,
                    output_channels: state.output_channels,
                    ..Default::default()
                };
            }
        }
        state
    }
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
mod tests;
