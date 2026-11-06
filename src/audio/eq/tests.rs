use super::{new_eq_handle, Equalizer};
use crate::models::EqSettings;
use rodio::source::SineWave;

#[test]
fn live_updates_wait_for_1024_frames_at_each_channel_count() {
    use rodio::{buffer::SamplesBuffer, ChannelCount, SampleRate};

    for channels in [1, 2, 6] {
        let interval = 1024 * usize::from(channels);
        let handle = new_eq_handle(EqSettings::default());
        let source = SamplesBuffer::new(
            ChannelCount::new(channels).unwrap(),
            SampleRate::new(48_000).unwrap(),
            vec![0.25; interval + 16],
        );
        let mut equalizer = Equalizer::new(source, handle.clone());
        let mut settings = EqSettings {
            enabled: true,
            ..Default::default()
        };
        settings.gains_db[5] = 12.0;
        handle.set(settings);

        for _ in 0..interval - 1 {
            assert_eq!(equalizer.next(), Some(0.25));
        }
        let first_updated_sample = equalizer.next().unwrap();
        assert!(first_updated_sample.is_finite());
        assert!((first_updated_sample - 0.25).abs() > 0.001);
    }
}

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
fn non_finite_gains_preserve_a_finite_signal() {
    for gain in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut eq = eq_with_gains(sine(1_000.0), [gain; 10]);
        for index in 0..2_000 {
            let value = eq.next().unwrap();
            assert!(
                value.is_finite(),
                "gain={gain} sample={index} value={value}"
            );
            let expected = (std::f64::consts::TAU * index as f64 * 1_000.0 / 48_000.0).sin() as f32;
            assert!((value - expected).abs() < 1e-3);
        }
    }
}

#[test]
fn live_invalid_gains_do_not_poison_filter_state_or_later_changes() {
    let handle = new_eq_handle(EqSettings {
        enabled: true,
        gains_db: [0.0; 10],
    });
    let mut eq = Equalizer::new(sine(1_000.0), handle.clone());
    for _ in 0..64 {
        assert!(eq.next().unwrap().is_finite());
    }
    {
        let (_, mut settings) = handle.snapshot();
        settings.gains_db[5] = f32::NAN;
        handle.set(settings);
    }
    for _ in 0..2_048 {
        assert!(eq.next().unwrap().is_finite());
    }
    {
        let (_, mut settings) = handle.snapshot();
        settings.gains_db[5] = 12.0;
        handle.set(settings);
    }
    let samples: Vec<f32> = eq.take(4_096).collect();
    assert!(samples.iter().all(|sample| sample.is_finite()));
    assert!(steady_rms(&samples) > 2.5);
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
        let (_, mut settings) = handle.snapshot();
        settings.gains_db[0] = 12.0;
        handle.set(settings);
    }
    // 检查周期（1024 帧）过后新系数生效。
    let samples: Vec<f32> = (0..4_096).map(|_| eq.next().unwrap()).collect();
    let tail = steady_rms(&samples);
    assert!(tail > 1.5, "版本变更后新系数应生效：{tail}");
}
