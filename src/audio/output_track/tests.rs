use super::*;
use crate::audio::buffered_stream::tests::from_samples;
use crate::audio::eq::{new_eq_handle, Equalizer};
use crate::audio::error::{FailureError, StreamFailure};
use crate::models::EqSettings;

fn prepared(
    samples: Vec<f32>,
    channels: u16,
    rate: u32,
    duration: Option<Duration>,
) -> PreparedStream {
    let (source, error) = from_samples(
        samples,
        ChannelCount::new(channels).unwrap(),
        SampleRate::new(rate).unwrap(),
        duration,
    );
    PreparedStream {
        source,
        duration,
        error,
        buffer_progress: Arc::new(BufferProgress::new(25, Some(100))),
    }
}

#[test]
fn output_keeps_shared_stream_diagnostics_and_exposes_only_the_requested_format() {
    let duration = Some(Duration::from_secs(1));
    let stream = prepared(vec![0.25; 8192], 1, 8_000, duration);
    let error = stream.error.clone();
    let progress = stream.buffer_progress.clone();
    let control = stream.source.control().clone();
    let track = OutputTrack::new(
        stream,
        2.0,
        ChannelCount::new(2).unwrap(),
        SampleRate::new(48_000).unwrap(),
        new_eq_handle(EqSettings::default()),
    );
    assert_eq!(track.info.duration, duration);
    assert!(Arc::ptr_eq(&track.info.error, &error));
    assert!(Arc::ptr_eq(&track.info.buffer, &progress));
    assert!(Arc::ptr_eq(&track.info.control, &control));
    assert!(control.is_speculative());
    assert!(!control.fully_decoded());
    assert!(control.check().is_ok());
    assert_eq!(*track.info.loudness.lock().unwrap(), None);
    let source = track.into_source();
    assert_eq!(source.channels().get(), 2);
    assert_eq!(source.sample_rate().get(), 48_000);
    assert_eq!(source.total_duration(), duration);
    drop(source);
    assert!(control.check().is_err());
}

#[test]
fn output_drop_cancels_the_stream_even_while_diagnostics_are_retained() {
    for consumed in [0, 1, 8192] {
        let stream = prepared(vec![0.5; 8192], 2, 48_000, None);
        let mut track = OutputTrack::new(
            stream,
            1.0,
            ChannelCount::new(2).unwrap(),
            SampleRate::new(48_000).unwrap(),
            new_eq_handle(EqSettings::default()),
        );
        let info = track.info.clone();
        for _ in 0..consumed {
            assert_eq!(track.next_sample(), Some(0.5));
        }
        assert!(info.control.check().is_ok());
        drop(track);
        assert!(info.control.check().is_err());
        assert!(info.error.lock().unwrap().is_none());
        assert_eq!(info.buffer.cached_bytes(), 25);
    }
}

#[test]
fn pipeline_matches_the_previous_inline_order_and_gain_clamping_sample_for_sample() {
    let settings = EqSettings {
        enabled: true,
        gains_db: [3.0, -2.0, 1.0, 0.0, -1.0, 4.0, 2.0, -3.0, 1.0, 0.0],
    };
    for (channels, rate, output_channels, output_rate) in [
        (1, 8_000, 2, 48_000),
        (2, 44_100, 2, 48_000),
        (6, 96_000, 1, 44_100),
    ] {
        let input: Vec<_> = [0.0, 0.25, -0.5, 1.5, -2.0, 0.125]
            .into_iter()
            .cycle()
            .take(8192)
            .collect();
        for gain in [0.0_f32, 1.0, 5.0, f32::NAN] {
            let format_channels = ChannelCount::new(output_channels).unwrap();
            let format_rate = SampleRate::new(output_rate).unwrap();
            let track = OutputTrack::new(
                prepared(input.clone(), channels, rate, None),
                gain,
                format_channels,
                format_rate,
                new_eq_handle(settings.clone()),
            );
            let reference = prepared(input.clone(), channels, rate, None);
            let metered = LoudnessMeter::new(reference.source, Loudness::default());
            let equalized = Equalizer::new(
                metered.amplify(gain.clamp(0.1, 4.0)),
                new_eq_handle(settings.clone()),
            );
            let expected: Vec<_> = normalize_for_output(equalized, format_channels, format_rate)
                .map(f32::to_bits)
                .collect();
            let actual: Vec<_> = track.into_source().map(f32::to_bits).collect();
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn raw_loudness_is_independent_of_gain_equalization_and_output_conversion() {
    let input: Vec<_> = (0..32_768)
        .map(|index| {
            (0.25 * (std::f64::consts::TAU * (index / 2) as f64 * 1_000.0 / 48_000.0).sin()) as f32
        })
        .collect();
    let sum_squares: f64 = input.iter().map(|sample| f64::from(*sample).powi(2)).sum();
    let expected_db = (20.0 * (sum_squares / input.len() as f64).sqrt().log10()) as f32;
    let expected_peak = input
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    for (gain, eq_gain, channels, rate) in [
        (1.0, 0.0, 2, 48_000),
        (4.0, 12.0, 2, 96_000),
        (0.1, -12.0, 1, 44_100),
    ] {
        let mut settings = EqSettings {
            enabled: true,
            ..Default::default()
        };
        settings.gains_db[5] = eq_gain;
        let track = OutputTrack::new(
            prepared(input.clone(), 2, 48_000, None),
            gain,
            ChannelCount::new(channels).unwrap(),
            SampleRate::new(rate).unwrap(),
            new_eq_handle(settings),
        );
        let loudness = track.info.loudness.clone();
        assert!(track.into_source().count() > 0);
        let (db, peak) = loudness.lock().unwrap().unwrap();
        assert!((db - expected_db).abs() < 1e-5);
        assert_eq!(peak, expected_peak);
    }
}

#[test]
fn reader_failure_reaches_shared_diagnostics_after_the_processed_samples_end() {
    let stream = prepared(vec![0.25; 8192], 2, 48_000, None);
    stream.source.control().record_io_error(FailureError::new(
        StreamFailure::Timeout,
        "reader stopped responding",
    ));
    let mut track = OutputTrack::new(
        stream,
        1.0,
        ChannelCount::new(2).unwrap(),
        SampleRate::new(48_000).unwrap(),
        new_eq_handle(EqSettings::default()),
    );
    let info = track.info.clone();
    while let Some(sample) = track.next_sample() {
        assert_eq!(sample, 0.25);
    }
    let error = info.error.lock().unwrap().clone().unwrap();
    assert_eq!(error.kind, StreamFailure::Timeout);
    assert_eq!(error.detail, "reader stopped responding");
    assert!(!info.control.fully_decoded());
}
