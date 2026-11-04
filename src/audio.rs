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

/// 播放失败的结构化原因，用来在界面上区分「本地网络」「服务端」「解码」三类问题。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamFailure {
    /// 无法建立连接：DNS 失败、拒绝连接、无路由。
    Connect,
    /// 连接已建立但迟迟没有响应，或读取中途超时。
    Timeout,
    /// 服务端返回了错误状态码。
    Http(u16),
    /// 连接还在，但长时间没有音频数据到达。
    NoData,
    /// 数据提前结束，通常是代理/转码截断了响应。
    Truncated,
    /// 下载到的内容无法解码。
    Decode,
    /// 本地音频输出设备问题。
    Output,
}

impl StreamFailure {
    /// 界面上的短标题（翻译键）。
    pub fn summary(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Connect => t!("Cannot reach the server"),
            Self::Timeout => t!("Server is not responding"),
            Self::Http(_) => t!("Server returned an error"),
            Self::NoData => t!("Server stopped sending audio"),
            Self::Truncated => t!("Audio stream was cut off"),
            Self::Decode => t!("Cannot decode this track"),
            Self::Output => t!("Audio output failed"),
        }
    }

    /// 归因提示：帮助用户判断是网络还是服务端的问题（翻译键）。
    pub fn cause(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Connect => t!("Networking: the server could not be reached. Check Wi-Fi/VPN and the server URL."
            ),
            Self::Timeout => t!("Networking or server: the connection opened but the reply timed out."
            ),
            Self::Http(status) if status >= 500 => t!("Server: the server reported an internal error. Retry later."
            ),
            Self::Http(_) => t!("Server: the request was rejected by the server."),
            Self::NoData => t!("Server: the connection is alive but no audio is arriving (transcoding or overloaded)."
            ),
            Self::Truncated => t!("Networking or server: the stream ended early, usually a proxy or idle timeout."
            ),
            Self::Decode => t!("Client: the received data is not decodable audio."),
            Self::Output => t!("Local: the audio device failed; unrelated to the network."),
        }
    }

    /// 是否可以自动重连。网络抖动、超时、服务端 5xx、流被截断都值得重试；
    /// 解码失败与本地输出问题重试无意义。
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::Connect | Self::Timeout | Self::NoData | Self::Truncated
        ) || matches!(self, Self::Http(status) if status >= 500 || status == 408 || status == 429)
    }
}

/// 携带分类的播放错误；`Display` 只输出人类可读的细节，分类通过 downcast 取回。
#[derive(Clone, Debug)]
pub(crate) struct FailureError {
    pub(crate) kind: StreamFailure,
    pub(crate) detail: String,
}

impl FailureError {
    fn new(kind: StreamFailure, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    /// 完整的单行描述，形如「Server stopped sending audio: no data for 30s」。
    pub(crate) fn message(&self) -> String {
        format!("{}: {}", self.kind.summary(), self.detail)
    }
}

impl std::fmt::Display for FailureError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for FailureError {}

/// 把带分类的错误包成 `io::Error`，让 reader 的调用链不丢失原因。
fn failure_io(kind: StreamFailure, detail: impl Into<String>) -> io::Error {
    io::Error::other(FailureError::new(kind, detail))
}

fn classify_reqwest(error: &reqwest::Error) -> StreamFailure {
    if let Some(status) = error.status() {
        return StreamFailure::Http(status.as_u16());
    }
    if error.is_timeout() {
        return StreamFailure::Timeout;
    }
    if error.is_connect() {
        return StreamFailure::Connect;
    }
    if error.is_body() || error.is_decode() {
        return StreamFailure::Truncated;
    }
    StreamFailure::Connect
}

fn classify_io(error: &io::Error) -> StreamFailure {
    if let Some(failure) = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<FailureError>())
    {
        return failure.kind;
    }
    match error.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => StreamFailure::Timeout,
        io::ErrorKind::ConnectionRefused
        | io::ErrorKind::ConnectionReset
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::NotConnected
        | io::ErrorKind::AddrNotAvailable => StreamFailure::Connect,
        io::ErrorKind::UnexpectedEof => StreamFailure::Truncated,
        _ => StreamFailure::Decode,
    }
}

/// 从 anyhow 错误里尽可能还原分类。
fn classify_anyhow(error: &anyhow::Error) -> StreamFailure {
    if let Some(failure) = error.downcast_ref::<FailureError>() {
        return failure.kind;
    }
    if let Some(io) = error.downcast_ref::<io::Error>() {
        return classify_io(io);
    }
    if let Some(reqwest) = error.downcast_ref::<reqwest::Error>() {
        return classify_reqwest(reqwest);
    }
    let text = format!("{error:#}").to_ascii_lowercase();
    if text.contains("timed out") || text.contains("timeout") {
        StreamFailure::Timeout
    } else if text.contains("connect") || text.contains("dns") || text.contains("resolve") {
        StreamFailure::Connect
    } else {
        StreamFailure::Decode
    }
}

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
