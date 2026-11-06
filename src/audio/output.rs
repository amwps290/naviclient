//! Audio output device discovery, stream opening, and format normalization.

use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Result};
use rodio::{
    cpal::BufferSize, source::UniformSourceIterator, ChannelCount, DeviceSinkBuilder, SampleRate,
    Source,
};

use super::{set_state, PlaybackState};

const OUTPUT_BUFFER_FRAMES: u32 = 4096;

/// 可选择的输出设备，保留平台原始名称以供配置和设备打开时匹配。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputDevice {
    pub name: String,
    pub is_default: bool,
}

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
/// plughw 带格式转换层，任意采样率/声道更稳健，打开同名设备时优先使用。
fn is_plughw_device(device: &cpal::Device) -> bool {
    use cpal::traits::DeviceTrait;
    device
        .id()
        .map(|id| id.1.starts_with("plughw:"))
        .unwrap_or(false)
}

/// 枚举系统输出设备，按名称去重（同名 hw/plughw
/// 只保留一条）。在后台线程调用，避免阻塞 UI。
pub fn list_output_devices() -> Result<Vec<OutputDevice>> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    let default_name = host
        .default_output_device()
        .and_then(|device| device.description().ok())
        .map(|description| description.name().to_string());
    let devices = host
        .output_devices()
        .map_err(|error| anyhow!("could not enumerate output devices: {error}"))?;
    let names = devices.filter_map(|device| {
        device
            .description()
            .ok()
            .map(|description| description.name().to_string())
    });
    Ok(output_device_list(names, default_name.as_deref()))
}

fn output_device_list(
    names: impl IntoIterator<Item = String>,
    default_name: Option<&str>,
) -> Vec<OutputDevice> {
    let mut devices: Vec<OutputDevice> = Vec::new();
    for name in names {
        if devices.iter().any(|device| device.name == name) {
            continue;
        }
        let is_default = default_name == Some(name.as_str());
        devices.push(OutputDevice { name, is_default });
    }
    devices
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
        // 设置页按名称去重；打开同名条目（hw/plughw）时优先使用
        // 带格式转换的 plughw。
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_names_are_deduplicated_in_platform_order_and_default_uses_exact_name() {
        let names = ["DAC", "Speakers", "DAC", "speakers"].map(str::to_string);
        assert_eq!(
            output_device_list(names, Some("Speakers")),
            vec![
                OutputDevice {
                    name: "DAC".into(),
                    is_default: false
                },
                OutputDevice {
                    name: "Speakers".into(),
                    is_default: true
                },
                OutputDevice {
                    name: "speakers".into(),
                    is_default: false
                },
            ]
        );
    }

    #[test]
    fn absent_default_does_not_add_a_device_or_mark_an_unrelated_device() {
        for default in [None, Some("disconnected")] {
            assert_eq!(
                output_device_list([" USB DAC ".into()], default),
                vec![OutputDevice {
                    name: " USB DAC ".into(),
                    is_default: false
                },]
            );
        }
        assert!(output_device_list(Vec::new(), Some("disconnected")).is_empty());
    }
}
