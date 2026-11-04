//! Audio output device discovery, stream opening, format normalization, and loudness metering.

use super::*;
use rodio::{cpal::BufferSize, source::UniformSourceIterator, DeviceSinkBuilder};

const OUTPUT_BUFFER_FRAMES: u32 = 4096;

/// 向状态上报当前输出设备与实际采样率/声道（诊断展示用）。
pub(super) fn report_output_device(
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
pub(super) fn open_audio_device(
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

pub(super) struct AudioOutput {
    /// 保持存活的输出流：切换设备时整体替换，旧 sink 随之 drop 关闭。
    /// 测试（无声卡音频工作线程）传 None。
    pub(super) sink: Option<rodio::MixerDeviceSink>,
    /// 当前设备名；None 表示跟随系统默认。
    pub(super) name: Option<String>,
    pub(super) player: Player,
    pub(super) channels: ChannelCount,
    pub(super) sample_rate: SampleRate,
    pub(super) handoff: gapless::Slot,
    /// 均衡器共享状态（设置 + 版本号）。
    pub(super) eq: EqHandle,
}

pub(super) fn normalize_for_output<S>(
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
pub(super) struct LoudnessMeter<S> {
    inner: S,
    sum_squares: f64,
    sample_count: u64,
    peak: f32,
    shared: Arc<Mutex<Option<(f32, f32)>>>,
}

impl<S: Source> LoudnessMeter<S> {
    pub(super) fn new(inner: S, shared: Arc<Mutex<Option<(f32, f32)>>>) -> Self {
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
