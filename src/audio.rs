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
use rust_i18n::t;
use url::Url;

use crate::models::EqSettings;

mod preload;
use preload::{Preloader, StreamControl};
mod gapless;
pub use gapless::TrackTransition;
use gapless::{ArmedTrack, AutoTrack, GaplessSource, OutputTrack, Transition};
mod eq;
use eq::{new_eq_handle, EqHandle};

const STREAM_BUFFER_BYTES: usize = 1024 * 1024;
const DECODED_CHUNK_FRAMES: usize = 4096;
const DECODED_BUFFER_CHUNKS: usize = 512;
const STARTUP_BUFFER_CHUNKS: usize = 16;
const STARTUP_BUFFER_TIMEOUT: Duration = Duration::from_secs(20);
const STREAM_STALL_TIMEOUT: Duration = Duration::from_secs(20);
/// 音频数据停止流入多久后在界面上显示「缓冲中」。
const STALL_VISIBLE_AFTER: Duration = Duration::from_secs(3);
/// 一条曲目在一次播放中最多自动重连几次（用尽后向用户报错）。
pub(crate) const PLAYBACK_RECONNECT_ATTEMPTS: u8 = 2;
const HTTP_IO_TIMEOUT: Duration = Duration::from_secs(12);
const OUTPUT_BUFFER_FRAMES: u32 = 4096;
const HTTP_READ_RETRIES: usize = 3;
const TRANSCODE_END_RETRIES: usize = 1;
const SEEK_DEBOUNCE: Duration = Duration::from_millis(180);
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

#[derive(Debug)]
enum Command {
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

fn apply_playback_intent(player: &Player, paused: bool) {
    if paused {
        player.pause();
    } else {
        player.play();
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

fn run_worker(
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

/// 向状态上报当前输出设备与实际采样率/声道（诊断展示用）。
fn report_output_device(
    state: &Arc<Mutex<PlaybackState>>,
    name: &Option<String>,
    sink: &rodio::MixerDeviceSink,
) {
    let sample_rate = sink.config().sample_rate().get();
    let channels = sink.config().channel_count().get();
    set_state(state, |s| {
        s.output_name = name.clone();
        s.output_sample_rate = sample_rate;
        s.output_channels = channels;
    });
}

/// 判断是否为 ALSA 的 plughw 条目：cpal 的 ALSA 后端会把同一物理 PCM
/// 枚举为 hw:/plughw: 两个设备，描述第一行（即显示名）完全相同；
/// plughw 带格式转换层，任意采样率/声道更稳健，去重时优先保留。
fn is_plughw_device(device: &cpal::Device) -> bool {
    use cpal::traits::DeviceTrait;
    device
        .id()
        .map(|id| id.1.starts_with("plughw:"))
        .unwrap_or(false)
}

/// 枚举系统输出设备：(名称, 是否默认)，按名称去重（同名 hw/plughw
/// 只保留一条）。在后台线程调用，避免阻塞 UI。
pub fn list_output_devices() -> Result<Vec<(String, bool)>> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let default_name = host
        .default_output_device()
        .and_then(|device| device.description().ok())
        .map(|description| description.name().to_string());
    let devices = host
        .output_devices()
        .map_err(|error| anyhow!("could not enumerate output devices: {error}"))?;
    let mut seen: Vec<(String, bool)> = Vec::new();
    for device in devices {
        let Ok(description) = device.description() else {
            continue;
        };
        let name = description.name().to_string();
        let plughw = is_plughw_device(&device);
        match seen.iter_mut().find(|(seen_name, _)| *seen_name == name) {
            // 同名重复：hw 后遇到 plughw 时升级标记，否则跳过。
            Some((_, prefer_plughw)) => *prefer_plughw |= plughw,
            None => seen.push((name, plughw)),
        }
    }
    Ok(seen
        .into_iter()
        .map(|(name, _)| {
            let is_default = default_name.as_deref() == Some(name.as_str());
            (name, is_default)
        })
        .collect())
}

/// 按名称打开指定输出设备；None 或设备不存在时回退系统默认。
/// 返回实际打开的 sink 与设备名（回退后名称与请求可能不同）。
fn open_audio_device(
    name: Option<&str>,
) -> Result<(rodio::MixerDeviceSink, Option<String>), anyhow::Error> {
    let Some(name) = name else {
        let sink = open_audio_sink().map_err(|error| anyhow!("{error}"))?;
        return Ok((sink, None));
    };
    use cpal::traits::{DeviceTrait, HostTrait};
    let devices = cpal::default_host()
        .output_devices()
        .map_err(|error| anyhow!("could not enumerate output devices: {error}"))?
        // 同名条目（hw/plughw）优先打开带格式转换的 plughw，
        // 与设置页列表的去重规则保持一致。
        .filter(|device| {
            device
                .description()
                .map(|description| description.name() == name)
                .unwrap_or(false)
        });
    let mut fallback = None;
    for device in devices {
        if is_plughw_device(&device) {
            return open_named_sink(device, name);
        }
        fallback.get_or_insert(device);
    }
    if let Some(device) = fallback {
        return open_named_sink(device, name);
    }
    // 目标设备不存在（如已拔出）：回退默认设备，保证播放不中新。
    log::warn!("audio output device {name:?} not found; falling back to the default device");
    let sink = open_audio_sink().map_err(|error| anyhow!("{error}"))?;
    Ok((sink, None))
}

/// 按设备打开输出流；返回 sink 与设备名。
fn open_named_sink(
    device: cpal::Device,
    name: &str,
) -> Result<(rodio::MixerDeviceSink, Option<String>), anyhow::Error> {
    let builder = DeviceSinkBuilder::from_device(device)
        .map_err(|error| anyhow!("{error}"))?
        .with_buffer_size(BufferSize::Fixed(OUTPUT_BUFFER_FRAMES));
    let sink = builder
        .open_sink_or_fallback()
        .map_err(|error| anyhow!("{error}"))?;
    Ok((sink, Some(name.to_string())))
}

struct AudioOutput {
    /// 保持存活的输出流：切换设备时整体替换，旧 sink 随之 drop 关闭。
    /// 测试（无声卡音频工作线程）传 None。
    sink: Option<rodio::MixerDeviceSink>,
    /// 当前设备名；None 表示跟随系统默认。
    name: Option<String>,
    player: Player,
    channels: ChannelCount,
    sample_rate: SampleRate,
    handoff: gapless::Slot,
    /// 均衡器共享状态（设置 + 版本号）。
    eq: EqHandle,
}

fn run_playback(
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

/// 测量播放中音频响度（RMS，dB）与峰值的 Source 包装；
/// 采样经由此处流入播放器，结果周期性写入共享槽供 worker 在曲目结束时读取。
struct LoudnessMeter<S> {
    inner: S,
    sum_squares: f64,
    sample_count: u64,
    peak: f32,
    shared: Arc<Mutex<Option<(f32, f32)>>>,
}

impl<S: Source> LoudnessMeter<S> {
    fn new(inner: S, shared: Arc<Mutex<Option<(f32, f32)>>>) -> Self {
        Self {
            inner,
            sum_squares: 0.0,
            sample_count: 0,
            peak: 0.0,
            shared,
        }
    }
}

impl<S: Source> Iterator for LoudnessMeter<S> {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        let sample = self.inner.next()?;
        let value = f64::from(sample);
        self.sum_squares += value * value;
        self.sample_count += 1;
        self.peak = self.peak.max(sample.abs());
        // 每 4096 个采样（约 93ms @44.1kHz）更新一次共享读数。
        if self.sample_count & 4095 == 0 {
            let rms = (self.sum_squares / self.sample_count as f64).sqrt();
            let db = if rms > 1e-6 {
                20.0 * rms.log10()
            } else {
                -100.0
            };
            if let Ok(mut shared) = self.shared.lock() {
                *shared = Some((db as f32, self.peak));
            }
        }
        Some(sample)
    }
}

impl<S: Source> Source for LoudnessMeter<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
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
    control: Arc<StreamControl>,
) -> Result<PreparedStream> {
    log::debug!(
        "preparing audio stream; endpoint={} start_ms={}",
        stream_endpoint(url),
        start_position.as_millis()
    );
    let reader = HttpRangeReader::open_controlled(
        client,
        url.to_string(),
        cache_dir,
        cache_key,
        control.clone(),
    )?;
    let buffer_progress = Arc::clone(&reader.buffer_progress);
    let byte_len = reader.len();
    let seekable = reader.is_seekable();
    let mime_type = reader.mime_type().map(str::to_owned);
    log::debug!("audio reader ready; byte_len={byte_len:?} seekable={seekable} mime={mime_type:?}");
    let buffered = BufReader::with_capacity(STREAM_BUFFER_BYTES, reader);

    let mut builder = Decoder::builder()
        .with_data(buffered)
        .with_seekable(seekable)
        .with_gapless(true);
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
    let (source, stream_error) = BufferedStreamSource::new(decoder, remaining_duration, control)?;
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
    Error(FailureError),
}

struct BufferedStreamSource {
    control: Arc<StreamControl>,
    receiver: Receiver<StreamMessage>,
    queued: VecDeque<Vec<Sample>>,
    current: std::vec::IntoIter<Sample>,
    channels: ChannelCount,
    sample_rate: SampleRate,
    duration: Option<Duration>,
    producer_finished: bool,
    terminal_error: Option<FailureError>,
    shared_error: SharedStreamError,
    last_data: Instant,
    silence_remaining: usize,
}

impl BufferedStreamSource {
    fn new(
        decoder: Decoder<BufReader<HttpRangeReader>>,
        duration: Option<Duration>,
        control: Arc<StreamControl>,
    ) -> Result<(Self, SharedStreamError)> {
        let channels = decoder.channels();
        let sample_rate = decoder.sample_rate();
        let (sender, receiver) = mpsc::sync_channel(DECODED_BUFFER_CHUNKS);
        let shared_error = Arc::new(Mutex::new(None));

        let decoder_control = control.clone();
        let decoder_guard = control.decoder_guard();
        thread::Builder::new()
            .name("audio-decoder".to_string())
            .spawn(move || {
                let _guard = decoder_guard;
                decode_stream(
                    decoder,
                    sender,
                    duration,
                    channels,
                    sample_rate,
                    decoder_control,
                )
            })
            .context("failed to start the audio decoder thread")?;

        let mut queued = VecDeque::new();
        let mut producer_finished = false;
        let mut terminal_error = None;
        let deadline = Instant::now() + STARTUP_BUFFER_TIMEOUT;

        while queued.len() < STARTUP_BUFFER_CHUNKS && !producer_finished {
            control.check()?;
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            match receiver.recv_timeout(remaining.min(Duration::from_millis(100))) {
                Ok(StreamMessage::Samples(samples)) => queued.push_back(samples),
                Ok(StreamMessage::End) => producer_finished = true,
                Ok(StreamMessage::Error(error)) => {
                    terminal_error = Some(error);
                    producer_finished = true;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    terminal_error = Some(FailureError::new(
                        StreamFailure::Decode,
                        "audio decoder stopped unexpectedly",
                    ));
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
            return Err(anyhow!(FailureError::new(
                StreamFailure::NoData,
                t!(
                    "no audio arrived within %{seconds}s while buffering",
                    seconds = STARTUP_BUFFER_TIMEOUT.as_secs()
                )
                .to_string(),
            )));
        }
        log::info!(
            "audio startup buffer ready; chunks={} target_chunks={} producer_finished={producer_finished}",
            queued.len(),
            STARTUP_BUFFER_CHUNKS
        );

        let source = Self {
            control,
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

    fn fail(&mut self, error: FailureError) -> Option<Sample> {
        self.producer_finished = true;
        self.terminal_error = Some(error);
        self.finish()
    }
}

impl Drop for BufferedStreamSource {
    fn drop(&mut self) {
        self.control.cancel();
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
                self.last_data = Instant::now();
                self.control.mark_data();
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
                    self.control.mark_data();
                    self.current = samples.into_iter();
                }
                Ok(StreamMessage::End) => self.producer_finished = true,
                Ok(StreamMessage::Error(error)) => {
                    // 解码器在 I/O 失败时会静默结束；reader 记下的原因更接近真相。
                    self.terminal_error = Some(self.control.take_io_error().unwrap_or(error));
                    self.producer_finished = true;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    if self.last_data.elapsed() >= STREAM_STALL_TIMEOUT {
                        let error = self.control.take_io_error().unwrap_or_else(|| {
                            FailureError::new(
                                StreamFailure::NoData,
                                t!(
                                    "no audio data for %{seconds}s while playback was running",
                                    seconds = STREAM_STALL_TIMEOUT.as_secs()
                                )
                                .to_string(),
                            )
                        });
                        return self.fail(error);
                    }
                    self.control.mark_synthetic_silence_frame(self.sample_rate);
                    self.silence_remaining = usize::from(self.channels.get()).saturating_sub(1);
                    return Some(0.0);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    return self.fail(FailureError::new(
                        StreamFailure::Decode,
                        "audio decoder stopped unexpectedly",
                    ));
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
    control: Arc<StreamControl>,
) {
    let samples_per_chunk = DECODED_CHUNK_FRAMES * usize::from(channels.get());
    let mut decoded_samples = 0_u64;
    log::debug!(
        "audio decoder thread running; channels={} sample_rate={} chunk_samples={samples_per_chunk}",
        channels.get(),
        sample_rate.get()
    );

    let mut chunks = 0;
    while control.wait_for_decode(chunks) {
        let samples: Vec<_> = decoder.by_ref().take(samples_per_chunk).collect();
        if samples.is_empty() {
            break;
        }
        decoded_samples = decoded_samples.saturating_add(samples.len() as u64);
        if sender.send(StreamMessage::Samples(samples)).is_err() {
            log::debug!("audio decoder stopped because playback source was dropped");
            return;
        }
        chunks += 1;
    }

    log::debug!(
        "audio decoder reached stream end; decoded_samples={decoded_samples} decoded_ms={}",
        decoded_duration(decoded_samples, channels, sample_rate).as_millis()
    );
    let message = if stream_ended_too_early(decoded_samples, channels, sample_rate, duration) {
        let decoded_duration = decoded_duration(decoded_samples, channels, sample_rate);
        StreamMessage::Error(FailureError::new(
            StreamFailure::Truncated,
            t!(
                "stream ended at %{actual}, expected about %{expected}",
                actual = format_playback(decoded_duration),
                expected = format_playback(duration.unwrap_or_default())
            )
            .to_string(),
        ))
    } else {
        StreamMessage::End
    };
    if matches!(message, StreamMessage::End) && control.check().is_ok() {
        control.fully_decoded.store(true, Ordering::Release);
    }
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
    control: Arc<StreamControl>,
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
    #[cfg(test)]
    fn open(client: Client, url: String, cache_dir: &Path, cache_key: &str) -> Result<Self> {
        Self::open_controlled(client, url, cache_dir, cache_key, StreamControl::new(false))
    }

    fn open_controlled(
        client: Client,
        url: String,
        cache_dir: &Path,
        cache_key: &str,
        control: Arc<StreamControl>,
    ) -> Result<Self> {
        control.check()?;
        let (complete_path, mut partial_path) = cache_paths(cache_dir, cache_key);
        if let Ok(file) = OpenOptions::new().read(true).open(&complete_path) {
            let len = file.metadata()?.len();
            log::info!(
                "audio cache hit; file={} bytes={len}",
                complete_path.display()
            );
            return Ok(Self {
                control,
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
        control.check()?;
        // Repeat-one and duplicate queue entries can preload the current song.
        // Never let a speculative reader modify its active reader's partial file.
        if control.speculative {
            static NEXT_PART: AtomicU64 = AtomicU64::new(0);
            let serial = NEXT_PART.fetch_add(1, Ordering::Relaxed);
            partial_path.set_extension(format!("preload-{}-{serial}.part", std::process::id()));
        }
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
            control,
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
        self.control.check()?;
        if position != 0 && !self.range_supported {
            return Err(failure_io(
                StreamFailure::Truncated,
                "audio server does not support HTTP range requests",
            ));
        }

        log::debug!(
            "reopening audio HTTP stream; endpoint={} byte_position={position}",
            stream_endpoint(&self.url)
        );
        let response = request_range(&self.client, &self.url, position).map_err(|error| {
            let io = io_error(error);
            self.control
                .record_io_error(FailureError::new(classify_io(&io), io.to_string()));
            io
        })?;
        if position != 0 && response.status() != StatusCode::PARTIAL_CONTENT {
            return Err(failure_io(
                StreamFailure::Truncated,
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
        self.control.check()?;
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
            self.control.check()?;
            let start = self.position;
            let result = self
                .response
                .as_mut()
                .ok_or_else(|| {
                    failure_io(StreamFailure::Connect, "audio HTTP response is unavailable")
                })?
                .read(buffer);
            self.control.check()?;
            match result {
                Ok(0) if self.near_estimated_end() => {
                    self.finish_estimated_stream();
                    return Ok(0);
                }
                Ok(0) if self.len.is_some_and(|len| self.position < len) => {
                    if !self.range_supported || attempt == HTTP_READ_RETRIES {
                        let error = failure_io(
                            StreamFailure::Truncated,
                            format!(
                                "stream ended at byte {} of {}",
                                self.position,
                                self.len.unwrap_or_default()
                            ),
                        );
                        self.control.record_io_error(FailureError::new(
                            StreamFailure::Truncated,
                            error.to_string(),
                        ));
                        return Err(error);
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
                        let kind = classify_io(&error);
                        log::warn!(
                            "audio stream read failed permanently; byte={} kind={kind:?}: {error}",
                            self.position
                        );
                        self.control
                            .record_io_error(FailureError::new(kind, error.to_string()));
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
        self.control.check()?;
        self.position = seek_target(self.position, self.len, from)?;
        Ok(self.position)
    }
}

impl Drop for HttpRangeReader {
    fn drop(&mut self) {
        let fully_cached = self
            .len
            .is_some_and(|len| len > 0 && self.cached_len >= len);
        if self.cache_complete {
            return;
        }
        self.cache_file.take();
        let unverified_preload =
            self.control.speculative && !self.control.fully_decoded.load(Ordering::Acquire);
        if !fully_cached || unverified_preload {
            if self.control.speculative {
                let _ = fs::remove_file(&self.partial_path);
            }
            return;
        }
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

fn request_range(client: &Client, url: &str, position: u64) -> Result<Response> {
    let response = client
        .get(url)
        .header(RANGE, format!("bytes={position}-"))
        .send()
        .map_err(|error| {
            anyhow!(FailureError::new(
                classify_reqwest(&error),
                t!(
                    "requesting the audio stream failed: %{error}",
                    error = error
                )
            ))
        })?;
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
    let status = response.status();
    if !status.is_success() {
        return Err(anyhow!(FailureError::new(
            StreamFailure::Http(status.as_u16()),
            t!("audio request returned HTTP %{status}", status = status)
        )));
    }
    // stream 端点正常必须返回音频二进制；HTTP 200 却给出 JSON/text
    // 说明服务器用 Subsonic 错误信封回应（例如拒绝/无法转码）。这里
    // 直接读出错误内容并中止，避免把错误正文喂给解码器后报出误导性的
    // “格式未识别”。归类为服务端拒绝，不触发自动重连。
    if response_is_error_envelope(&response) {
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let mut body = String::new();
        let _ = response
            .take(8 * 1024)
            .read_to_string(&mut body)
            .map_err(|error| format!("body read failed: {error}"));
        let detail = subsonic_error_detail(&body);
        log::warn!(
            "stream endpoint returned a non-audio response; endpoint={} status={status} content_type={} body={}",
            stream_endpoint(url),
            content_type,
            body.trim()
        );
        return Err(anyhow!(FailureError::new(
            StreamFailure::Http(status.as_u16()),
            t!(
                "server refused the stream request: %{detail}",
                detail = detail
            ),
        )));
    }
    Ok(response)
}

/// 判定响应是否为非音频的错误信封：stream 端点正常必须返回音频二进制，
/// JSON/text 正文只可能是协议错误（或完全跑错的网关页）。
fn response_is_error_envelope(response: &Response) -> bool {
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    content_type.starts_with("application/json")
        || content_type.starts_with("text/")
        || content_type.starts_with("application/xml")
}

/// 解析 Subsonic 错误信封，取出服务端给出的真实原因。
///
/// 形如 `{"subsonic-response":{"status":"failed","error":{"code":0,"message":"…"}}}`；
/// 解析不出时回退到截断后的原始正文，保证日志与界面仍能看到内容。
fn subsonic_error_detail(body: &str) -> String {
    let trimmed = body.trim();
    let raw = || {
        if trimmed.is_empty() {
            t!("the server returned an empty response").to_string()
        } else {
            let prefix: String = trimmed.chars().take(200).collect();
            if prefix.chars().count() < trimmed.chars().count() {
                format!("{prefix}…")
            } else {
                prefix
            }
        }
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return raw();
    };
    let error = value
        .get("subsonic-response")
        .and_then(|response| response.get("error"))
        .or_else(|| value.get("error"));
    let Some(error) = error else {
        return raw();
    };
    let code = error.get("code").and_then(serde_json::Value::as_i64);
    let message = error
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    match (code, message.is_empty()) {
        (Some(code), false) => t!(
            "%{message} (Subsonic error %{code})",
            message = message,
            code = code
        )
        .to_string(),
        (Some(code), true) => t!("Subsonic error %{code}", code = code).to_string(),
        (None, false) => message.to_string(),
        (None, true) => raw(),
    }
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

/// 把输出设备时钟换算成真实曲目位置。网络停滞时注入的静音会推进
/// `Player::get_pos()`，但它没有消费歌曲内容，因此必须扣除。
fn logical_playback_position(
    position_offset: Duration,
    output_position: Duration,
    synthetic_silence: Duration,
) -> Duration {
    position_offset.saturating_add(output_position.saturating_sub(synthetic_silence))
}

fn io_error(error: anyhow::Error) -> io::Error {
    match error.downcast::<FailureError>() {
        Ok(failure) => failure_io(failure.kind, failure.detail),
        Err(other) => io::Error::other(other),
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
mod tests {
    use super::*;

    #[test]
    fn subsonic_error_envelope_yields_the_server_message() {
        // 标准错误信封：取出服务端给出的真实原因与错误码。
        let body = r#"{"subsonic-response":{"status":"failed","version":"1.16.1","error":{"code":0,"message":"transcoding not configured"}}}"#;
        assert_eq!(
            subsonic_error_detail(body),
            "transcoding not configured (Subsonic error 0)"
        );
        // 只有错误码：仍可给出可辨识的描述。
        assert_eq!(
            subsonic_error_detail(r#"{"error":{"code":70}}"#),
            "Subsonic error 70"
        );
        // JSON 但非信封结构：回退原始正文。
        let gateway = r#"{"detail":"gateway panic"}"#;
        assert_eq!(subsonic_error_detail(gateway), gateway);
    }

    #[test]
    fn subsonic_error_envelope_falls_back_to_raw_text_when_unparseable() {
        // 非 JSON 正文：截断保留前 200 字符。
        let html = "<html>502 Bad Gateway</html>";
        assert_eq!(subsonic_error_detail(html), html);
        let long = "x".repeat(300);
        assert_eq!(
            subsonic_error_detail(&long),
            format!("{}…", "x".repeat(200))
        );
        // 空正文：给出占位描述而不是空字符串。
        assert_eq!(
            subsonic_error_detail(""),
            "the server returned an empty response"
        );
        assert_eq!(
            subsonic_error_detail("   "),
            "the server returned an empty response"
        );
    }

    #[test]
    fn network_failures_are_retryable_but_decode_and_output_are_not() {
        for kind in [
            StreamFailure::Connect,
            StreamFailure::Timeout,
            StreamFailure::NoData,
            StreamFailure::Truncated,
            StreamFailure::Http(503),
            StreamFailure::Http(429),
        ] {
            assert!(kind.is_retryable(), "{kind:?} should be retried");
        }
        for kind in [
            StreamFailure::Decode,
            StreamFailure::Output,
            StreamFailure::Http(404),
        ] {
            assert!(!kind.is_retryable(), "{kind:?} should not be retried");
        }
    }

    #[test]
    fn failure_messages_keep_the_summary_and_the_detail() {
        let failure =
            FailureError::new(StreamFailure::Http(502), "audio request returned HTTP 502");
        assert_eq!(
            failure.message(),
            "Server returned an error: audio request returned HTTP 502"
        );
        // 归因提示要能把网络与服务端区分开，而不是只说「失败」。
        assert!(StreamFailure::Connect.cause().starts_with("Networking"));
        assert!(StreamFailure::Http(500).cause().starts_with("Server"));
        assert!(StreamFailure::Output.cause().starts_with("Local"));
    }

    #[test]
    fn io_errors_carry_their_classification_through_the_reader_chain() {
        let wrapped = failure_io(StreamFailure::Truncated, "stream ended at byte 10 of 90");
        assert_eq!(classify_io(&wrapped), StreamFailure::Truncated);
        assert_eq!(
            classify_io(&io::Error::from(io::ErrorKind::TimedOut)),
            StreamFailure::Timeout
        );
        assert_eq!(
            classify_io(&io::Error::from(io::ErrorKind::ConnectionRefused)),
            StreamFailure::Connect
        );
        assert_eq!(
            classify_io(&io::Error::from(io::ErrorKind::UnexpectedEof)),
            StreamFailure::Truncated
        );
    }

    #[test]
    fn anyhow_wrapping_preserves_the_failure_kind() {
        let anyhow_error = anyhow!(FailureError::new(StreamFailure::NoData, "no audio"));
        assert_eq!(classify_anyhow(&anyhow_error), StreamFailure::NoData);
        let contexted = anyhow_error.context("failed to prepare stream");
        assert_eq!(classify_anyhow(&contexted), StreamFailure::NoData);
    }

    #[test]
    fn restoring_playback_sends_one_command_with_the_saved_position() {
        let (tx, rx) = mpsc::channel();
        let audio = AudioHandle {
            tx,
            state: Arc::new(Mutex::new(PlaybackState::default())),
            transition: Arc::new(Mutex::new(None)),
        };
        audio.play(
            "test".into(),
            "song".into(),
            Some(Duration::from_secs(180)),
            1.0,
            Duration::from_secs(42),
            None,
        );
        assert!(
            matches!(rx.recv().unwrap(), Command::Play { start_position, .. } if start_position == Duration::from_secs(42))
        );
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn prepared_audio_obeys_current_intent_instead_of_the_cleared_sink() {
        let (player, _source) = Player::new();
        player.stop();
        player.clear();
        assert!(player.is_paused());
        apply_playback_intent(&player, false);
        assert!(!player.is_paused());
        // Pause/resume commands arriving during preparation must win when ready.
        apply_playback_intent(&player, true);
        assert!(player.is_paused());
        apply_playback_intent(&player, false);
        assert!(!player.is_paused());
    }

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
    fn seek_coalescing_preserves_intervening_transport_commands() {
        let (tx, rx) = mpsc::channel();
        tx.send(Command::Pause).unwrap();
        tx.send(Command::Seek(Duration::from_secs(30))).unwrap();
        let mut pending = VecDeque::new();
        let (position, count) = coalesce_seek_commands(Duration::from_secs(10), &rx, &mut pending);
        assert_eq!((position, count), (Duration::from_secs(10), 1));
        assert!(matches!(pending.pop_front(), Some(Command::Pause)));
        assert!(
            matches!(rx.recv().unwrap(), Command::Seek(position) if position == Duration::from_secs(30))
        );
    }

    #[test]
    fn loudness_meter_reports_rms_and_peak() {
        struct Constant {
            value: f32,
            remaining: usize,
            sample_rate: SampleRate,
        }
        impl Iterator for Constant {
            type Item = f32;
            fn next(&mut self) -> Option<Self::Item> {
                if self.remaining == 0 {
                    None
                } else {
                    self.remaining -= 1;
                    Some(self.value)
                }
            }
        }
        impl Source for Constant {
            fn current_span_len(&self) -> Option<usize> {
                Some(self.remaining)
            }
            fn channels(&self) -> ChannelCount {
                ChannelCount::new(1).unwrap()
            }
            fn sample_rate(&self) -> SampleRate {
                self.sample_rate
            }
            fn total_duration(&self) -> Option<Duration> {
                None
            }
        }

        let shared = Arc::new(Mutex::new(None::<(f32, f32)>));
        // 振幅 0.5 的恒信号：RMS = 0.5 → ≈ -6.02 dB，峰值 = 0.5。
        let meter = LoudnessMeter::new(
            Constant {
                value: 0.5,
                remaining: 20_000,
                sample_rate: SampleRate::new(44_100).unwrap(),
            },
            shared.clone(),
        );
        // 排空迭代器，触发共享读数更新。
        let _: Vec<f32> = meter.collect();
        let (rms_db, peak) = shared.lock().unwrap().expect("meter should have reported");
        assert!((rms_db - 20.0 * 0.5f32.log10()).abs() < 0.1);
        assert!((peak - 0.5).abs() < 0.001);
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
    fn synthetic_stall_silence_does_not_advance_the_track_position() {
        let control = StreamControl::new(false);
        let sample_rate = SampleRate::new(1_000).unwrap();
        for _ in 0..2_500 {
            control.mark_synthetic_silence_frame(sample_rate);
        }

        let synthetic_silence = control.synthetic_silence_duration();
        assert_eq!(synthetic_silence, Duration::from_millis(2_500));
        assert_eq!(
            logical_playback_position(
                Duration::from_secs(40),
                Duration::from_millis(12_500),
                synthetic_silence,
            ),
            Duration::from_secs(50)
        );
        assert_eq!(
            logical_playback_position(
                Duration::from_secs(40),
                Duration::from_secs(1),
                Duration::from_secs(2),
            ),
            Duration::from_secs(40)
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
            control: StreamControl::new(false),
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
