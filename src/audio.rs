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
use rodio::{ChannelCount, Decoder, Player, Sample, SampleRate, Source};
use rust_i18n::t;
use url::Url;

use crate::models::EqSettings;

mod error;
pub(crate) use error::FailureError;
pub use error::StreamFailure;
use error::{classify_anyhow, classify_io, classify_reqwest, failure_io};
mod http_stream;
use http_stream::*;
mod decoder;
use decoder::*;
mod preload;
use preload::{Preloader, StreamControl};
mod gapless;
pub use gapless::TrackTransition;
use gapless::{ArmedTrack, AutoTrack, GaplessSource, OutputTrack, Transition};
mod eq;
use eq::{new_eq_handle, EqHandle};
mod output;
pub use output::list_output_devices;
use output::{
    normalize_for_output, open_audio_device, report_output_device, AudioOutput, LoudnessMeter,
};
mod worker;
#[cfg(test)]
use worker::{apply_playback_intent, coalesce_seek_commands, run_playback};
use worker::{run_worker, Command};

const STREAM_BUFFER_BYTES: usize = 1024 * 1024;
const DECODED_CHUNK_FRAMES: usize = 4096;
const DECODED_BUFFER_CHUNKS: usize = 512;
const STARTUP_BUFFER_CHUNKS: usize = 16;
const STARTUP_BUFFER_TIMEOUT: Duration = Duration::from_secs(20);
const STREAM_STALL_TIMEOUT: Duration = Duration::from_secs(20);
/// 一条曲目在一次播放中最多自动重连几次（用尽后向用户报错）。
pub(crate) const PLAYBACK_RECONNECT_ATTEMPTS: u8 = 2;
const HTTP_READ_RETRIES: usize = 3;
const TRANSCODE_END_RETRIES: usize = 1;
const MAX_AUDIO_CACHE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

type SharedStreamError = Arc<Mutex<Option<FailureError>>>;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlaybackState {
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
    tx: Sender<Command>,
    state: Arc<Mutex<PlaybackState>>,
    transition: Arc<Mutex<Option<Arc<Transition>>>>,
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
        })
    }

    pub fn play(
        &self,
        url: String,
        cache_key: String,
        duration_hint: Option<Duration>,
        gain: f32,
        start_position: Duration,
        preload_id: Option<u64>,
    ) {
        self.cancel_transition();
        self.transition.lock().unwrap().take();
        if self
            .tx
            .send(Command::Play {
                url,
                cache_key,
                duration_hint,
                start_position,
                gain,
                preload_id,
            })
            .is_err()
        {
            log::error!("failed to send play command: audio worker is unavailable");
        }
    }

    pub fn pause(&self) {
        let _ = self.tx.send(Command::Pause);
    }

    pub fn preload(
        &self,
        id: u64,
        url: String,
        cache_key: String,
        duration_hint: Option<Duration>,
        gain: f32,
    ) {
        let transition = Transition::new(id, duration_hint);
        *self.transition.lock().unwrap() = Some(transition.clone());
        let _ = self.tx.send(Command::Preload {
            id,
            url,
            cache_key,
            duration_hint,
            gain,
            transition,
        });
    }

    pub fn cancel_preload(&self) -> Option<TrackTransition> {
        let event = self.freeze_preload();
        let _ = self.tx.send(Command::CancelPreload);
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
        let _ = self.tx.send(Command::Resume);
    }

    pub fn stop(&self) {
        self.cancel_transition();
        self.transition.lock().unwrap().take();
        let _ = self.tx.send(Command::Stop);
    }

    pub fn set_volume(&self, volume: f32) {
        let _ = self.tx.send(Command::SetVolume(volume));
    }

    pub fn set_cache_directory(&self, cache_dir: PathBuf) {
        self.cancel_transition();
        if self.tx.send(Command::SetCacheDirectory(cache_dir)).is_err() {
            log::error!("failed to update cache directory: audio worker is unavailable");
        }
    }

    /// 切换输出设备；播放中会在新设备上从当前位置继续，暂停意图保留。
    pub fn set_output_device(&self, name: Option<String>) {
        self.cancel_transition();
        if self.tx.send(Command::SetOutputDevice(name)).is_err() {
            log::error!("failed to switch output device: audio worker is unavailable");
        }
    }

    /// 更新均衡器设置；音频线程内重算系数，正在播放的曲目在下一个
    /// 版本检查点（约千帧）生效。
    pub fn set_equalizer(&self, settings: EqSettings) {
        if self.tx.send(Command::SetEqualizer(settings)).is_err() {
            log::error!("failed to update equalizer: audio worker is unavailable");
        }
    }

    pub fn seek(&self, position: Duration) {
        self.cancel_transition();
        let _ = self.tx.send(Command::Seek(position));
    }

    pub fn state(&self) -> PlaybackState {
        let mut state = self
            .state
            .lock()
            .map(|state| state.clone())
            .unwrap_or_default();
        // The output callback can lead the worker by one poll. Never expose the
        // previous song's end/position/loudness as the newly started song.
        if let Some(transition) = self.transition.lock().unwrap().as_ref() {
            if transition.event().is_some() && state.transition_id != Some(transition.id) {
                state = PlaybackState {
                    active: true,
                    paused: state.paused,
                    duration: transition.duration,
                    transition_id: Some(transition.id),
                    seek_serial: state.seek_serial,
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
