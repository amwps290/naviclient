//! 10 段图形均衡器：基于 `biquad` 库的 RBJ peaking 滤波器级联
//! （DirectForm1，每通道独立状态）。插入位置在 ReplayGain 增益之后、
//! 输出归一化之前：音轨原生采样率下滤波、且不影响响度测量
//! （响度缓存必须与 EQ 无关）。
//!
//! 设置经共享状态 + 版本号传递：UI 只发命令，音频线程更新共享状态，
//! 滤波器约每千帧检查一次版本（48 kHz 下约 21ms），变更时热更新系数
//! 并保留滤波状态以减小切换暂态。

use std::sync::{Arc, Mutex};

use biquad::{Biquad, Coefficients, DirectForm1, Hertz, Type};
use rodio::Source;

use crate::models::EqSettings;

/// 10 段中心频率（与 `EqSettings::BANDS` 一致）。
const BANDS: [f32; 10] = [
    31.25, 62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];
/// 图形均衡的 Q 值（约 1 倍频程带宽）。
const BAND_Q: f32 = 1.41;
/// 每多少帧（采样 × 声道）检查一次共享设置版本。
const VERSION_CHECK_FRAMES: usize = 1024;

/// 均衡器的共享状态：设置 + 版本号（每次设置变更递增）。
pub(crate) struct EqShared {
    pub(crate) version: u64,
    pub(crate) settings: EqSettings,
}

pub(crate) type EqHandle = Arc<Mutex<EqShared>>;

/// 创建共享状态；音频线程与应用各持一端。
pub(crate) fn new_eq_handle(settings: EqSettings) -> EqHandle {
    Arc::new(Mutex::new(EqShared {
        version: 0,
        settings,
    }))
}

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
        let (version, settings) = {
            let guard = shared.lock().unwrap();
            (guard.version, guard.settings.clone())
        };
        let mut equalizer = Self {
            inner,
            shared,
            version,
            enabled: false,
            bands: BANDS.iter().map(|_| Band::new(channels)).collect(),
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
                settings.gains_db[index].clamp(-12.0, 12.0)
            } else {
                0.0
            };
            band.set_peaking(BANDS[index], gain_db, self.sample_rate);
        }
    }

    fn refresh_if_changed(&mut self) {
        let Ok(guard) = self.shared.lock() else {
            return;
        };
        if guard.version == self.version {
            return;
        }
        self.version = guard.version;
        let settings = guard.settings.clone();
        drop(guard);
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
mod tests {
    use super::*;
    use rodio::source::SineWave;

    /// SineWave：单声道、48 kHz、幅度 1.0。
    fn sine(freq: f32) -> SineWave {
        SineWave::new(freq)
    }

    fn eq_with_gains(source: SineWave, gains: [f32; 10]) -> Equalizer<SineWave> {
        let handle = new_eq_handle(EqSettings {
            enabled: true,
            gains_db: gains,
        });
        Equalizer::new(source, handle)
    }

    /// 取稳定段（跳过前 25% 滤波暂态）的 RMS。
    fn steady_rms(samples: &[f32]) -> f32 {
        let skip = samples.len() / 4;
        let mut sum = 0.0f64;
        for &value in &samples[skip..] {
            sum += f64::from(value) * f64::from(value);
        }
        (sum / (samples.len() - skip).max(1) as f64).sqrt() as f32
    }

    #[test]
    fn flat_eq_passes_signal_unchanged() {
        // 全 0 dB 时级联应为恒等（浮点误差内）。
        let mut eq = eq_with_gains(sine(1_000.0), [0.0; 10]);
        let samples: Vec<f32> = (0..2_000).map(|_| eq.next().unwrap()).collect();
        for (index, &value) in samples.iter().enumerate().skip(4) {
            let expected = (std::f64::consts::TAU * index as f64 * 1_000.0 / 48_000.0).sin() as f32;
            assert!(
                (value - expected).abs() < 1e-3,
                "index={index} value={value} expected={expected}"
            );
        }
    }

    #[test]
    fn bass_band_boosts_low_and_leaves_high_alone() {
        // 仅 31.25Hz 段 +12dB：31Hz 测试音应增强约 4 倍幅度，
        // 4kHz 音距段中心足够远，应几乎不变。
        let boosted_gains = [12.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        // 31Hz 一个周期约 1548 个采样：窗口必须远大于滤波器暂态（数个周期）。
        let mut low = eq_with_gains(sine(31.0), boosted_gains);
        let low_samples: Vec<f32> = (0..16_000).map(|_| low.next().unwrap()).collect();
        let low_rms = steady_rms(&low_samples);

        let mut high = eq_with_gains(sine(4_000.0), boosted_gains);
        let high_samples: Vec<f32> = (0..4_000).map(|_| high.next().unwrap()).collect();
        let high_rms = steady_rms(&high_samples);

        assert!(low_rms > 2.5, "低频应显著增强（约 12dB）：{low_rms}");
        assert!((high_rms - 0.70).abs() < 0.1, "高频应基本不变：{high_rms}");
    }

    #[test]
    fn treble_band_boosts_high_and_cuts_when_negative() {
        let mut high = eq_with_gains(
            sine(8_000.0),
            [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 12.0, 0.0],
        );
        let samples: Vec<f32> = (0..4_000).map(|_| high.next().unwrap()).collect();
        assert!(
            steady_rms(&samples) > 2.5,
            "8kHz 段应增强：{}",
            steady_rms(&samples)
        );

        let mut cut = eq_with_gains(
            sine(8_000.0),
            [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -12.0, 0.0],
        );
        let samples: Vec<f32> = (0..4_000).map(|_| cut.next().unwrap()).collect();
        assert!(
            steady_rms(&samples) < 0.25,
            "8kHz 段应衰减：{}",
            steady_rms(&samples)
        );
    }

    #[test]
    fn disabled_eq_is_a_passthrough() {
        let handle = new_eq_handle(EqSettings {
            enabled: false,
            gains_db: [12.0; 10],
        });
        let mut eq = Equalizer::new(sine(440.0), handle);
        for index in 0..256 {
            let value = eq.next().unwrap();
            let expected = (std::f64::consts::TAU * index as f64 * 440.0 / 48_000.0).sin() as f32;
            assert!((value - expected).abs() < 1e-3);
        }
    }

    #[test]
    fn version_change_updates_coefficients_live() {
        let handle = new_eq_handle(EqSettings {
            enabled: true,
            gains_db: [0.0; 10],
        });
        let mut eq = Equalizer::new(sine(31.0), handle.clone());
        for _ in 0..64 {
            eq.next().unwrap();
        }
        {
            let mut guard = handle.lock().unwrap();
            guard.settings.gains_db[0] = 12.0;
            guard.version += 1;
        }
        // 检查周期（1024 帧）过后新系数生效。
        let samples: Vec<f32> = (0..4_096).map(|_| eq.next().unwrap()).collect();
        let tail = steady_rms(&samples);
        assert!(tail > 1.5, "版本变更后新系数应生效：{tail}");
    }
}
