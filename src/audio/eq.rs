//! 10 段图形均衡器：基于 `biquad` 库的 RBJ peaking 滤波器级联
//! （DirectForm1，每通道独立状态）。插入位置在 ReplayGain 增益之后、
//! 输出归一化之前：音轨原生采样率下滤波、且不影响响度测量
//! （响度缓存必须与 EQ 无关）。
//!
//! 设置经共享状态 + 版本号传递：UI 只发命令，音频线程更新共享状态，
//! 滤波器约每千帧检查一次版本（48 kHz 下约 21ms），变更时热更新系数
//! 并保留滤波状态以减小切换暂态。

use biquad::{Biquad, Coefficients, DirectForm1, Hertz, Type};
use rodio::Source;

use crate::models::EqSettings;

mod shared;
pub(crate) use shared::{new_eq_handle, EqHandle};

/// 图形均衡的 Q 值（约 1 倍频程带宽）。
const BAND_Q: f32 = 1.41;
/// 每多少帧（采样 × 声道）检查一次共享设置版本。
const VERSION_CHECK_FRAMES: usize = 1024;

/// 0 dB 恒等系数：频段超出奈奎斯特（如低采样率音源上的 16 kHz 段）
/// 或参数非法时回退直通。
fn identity_coefficients() -> Coefficients<f32> {
    Coefficients {
        a1: 0.0,
        a2: 0.0,
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
    }
}

/// 单个频段：每通道一个 DirectForm1；系数热更新保留滤波状态。
struct Band {
    states: Vec<DirectForm1<f32>>,
}

impl Band {
    fn new(channels: usize) -> Self {
        Self {
            states: (0..channels)
                .map(|_| DirectForm1::new(identity_coefficients()))
                .collect(),
        }
    }

    /// 计算峰值滤波系数并热更新到各通道。
    fn set_peaking(&mut self, center_hz: f32, gain_db: f32, sample_rate: f32) {
        let coefficients = Coefficients::from_params(
            Type::PeakingEQ(gain_db),
            Hertz::from_hz(sample_rate).expect("sample rate must be positive"),
            Hertz::from_hz(center_hz).expect("band center must be positive"),
            BAND_Q,
        )
        .unwrap_or_else(|error| {
            log::debug!("eq band {center_hz}Hz fell back to bypass: {error:?}");
            identity_coefficients()
        });
        for state in &mut self.states {
            state.update_coefficients(coefficients);
        }
    }
}

/// 均衡器 Source 包装：关闭时直通，开启时逐样本通过 10 段级联。
pub(crate) struct Equalizer<S> {
    inner: S,
    shared: EqHandle,
    version: u64,
    enabled: bool,
    bands: Vec<Band>,
    channels: usize,
    sample_rate: f32,
    samples_since_check: usize,
    check_interval: usize,
    channel: usize,
}

impl<S: Source<Item = f32>> Equalizer<S> {
    pub(crate) fn new(inner: S, shared: EqHandle) -> Self {
        let channels = usize::from(inner.channels().get()).max(1);
        let sample_rate = inner.sample_rate().get() as f32;
        let (version, settings) = shared.snapshot();
        let mut equalizer = Self {
            inner,
            shared,
            version,
            enabled: false,
            bands: EqSettings::BANDS
                .iter()
                .map(|_| Band::new(channels))
                .collect(),
            channels,
            sample_rate,
            samples_since_check: 0,
            check_interval: VERSION_CHECK_FRAMES * channels,
            channel: 0,
        };
        equalizer.apply(&settings);
        equalizer
    }

    /// 按设置重算 10 段系数（热更新，保留滤波状态）；关闭时各段回 0 dB。
    fn apply(&mut self, settings: &EqSettings) {
        self.enabled = settings.enabled;
        for (index, band) in self.bands.iter_mut().enumerate() {
            let gain_db = if settings.enabled {
                EqSettings::normalize_gain(settings.gains_db[index])
            } else {
                0.0
            };
            band.set_peaking(EqSettings::BANDS[index], gain_db, self.sample_rate);
        }
    }

    fn refresh_if_changed(&mut self) {
        let Some((version, settings)) = self.shared.changed_since(self.version) else {
            return;
        };
        self.version = version;
        self.apply(&settings);
    }
}

impl<S: Source<Item = f32>> Iterator for Equalizer<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let sample = self.inner.next()?;
        let channel = self.channel;
        self.channel = (self.channel + 1) % self.channels;
        self.samples_since_check += 1;
        if self.samples_since_check >= self.check_interval {
            self.samples_since_check = 0;
            self.refresh_if_changed();
        }
        if !self.enabled {
            return Some(sample);
        }
        let mut value = sample;
        for band in &mut self.bands {
            value = band.states[channel].run(value);
        }
        Some(value)
    }
}

impl<S: Source<Item = f32>> Source for Equalizer<S> {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> rodio::ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        self.inner.total_duration()
    }
}

#[cfg(test)]
mod tests;
