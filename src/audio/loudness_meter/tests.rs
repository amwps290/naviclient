use super::*;
use rodio::buffer::SamplesBuffer;

fn source(samples: Vec<f32>, channels: u16, rate: u32) -> SamplesBuffer {
    SamplesBuffer::new(
        ChannelCount::new(channels).unwrap(),
        SampleRate::new(rate).unwrap(),
        samples,
    )
}

#[test]
fn updates_on_exact_raw_sample_boundaries_and_keeps_cumulative_rms_and_peak() {
    let shared = Loudness::default();
    let samples = [vec![0.5; 4096], vec![-1.0; 4096], vec![0.0; 4096]].concat();
    let mut meter = LoudnessMeter::new(source(samples, 2, 48_000), shared.clone());
    for count in 1..=12_288 {
        assert!(meter.next().is_some());
        if count < 4096 {
            assert_eq!(*shared.lock().unwrap(), None);
        } else {
            let (db, peak) = shared.lock().unwrap().unwrap();
            let expected_rms = if count < 8192 {
                0.5_f64
            } else if count < 12_288 {
                ((0.25_f64 + 1.0) / 2.0).sqrt()
            } else {
                ((0.25_f64 + 1.0) / 3.0).sqrt()
            };
            assert!((db - (20.0 * expected_rms.log10()) as f32).abs() < 1e-5);
            assert_eq!(peak, if count < 8192 { 0.5 } else { 1.0 });
        }
    }
    assert_eq!(meter.next(), None);
}

#[test]
fn silence_and_subthreshold_samples_keep_the_existing_minus_100_db_floor() {
    for value in [0.0_f32, -0.0, 1e-8, -1e-8, 1e-6, -1e-6] {
        let mut samples = SampleLoudness::default();
        for _ in 0..4095 {
            assert_eq!(samples.observe(value), None);
        }
        assert_eq!(samples.observe(value), Some((-100.0, value.abs())));
    }
    let mut samples = SampleLoudness::default();
    for _ in 0..4095 {
        samples.observe(2e-6);
    }
    let (db, peak) = samples.observe(2e-6).unwrap();
    assert!((db - 20.0 * 2e-6_f32.log10()).abs() < 1e-5);
    assert_eq!(peak, 2e-6);
}

#[test]
fn exhaustion_does_not_flush_short_tracks_or_publish_the_partial_tail() {
    for count in [0, 1, 4095, 4096, 4097, 8191] {
        let shared = Arc::new(Mutex::new(Some((-12.0, 0.25))));
        let mut input = vec![0.5; count];
        if count > 4096 {
            input[4096..].fill(1.0);
        }
        let mut meter = LoudnessMeter::new(source(input, 1, 44_100), shared.clone());
        assert_eq!(meter.by_ref().count(), count);
        let expected = if count < 4096 {
            (-12.0, 0.25)
        } else {
            ((20.0_f64 * 0.5_f64.log10()) as f32, 0.5)
        };
        assert_eq!(*shared.lock().unwrap(), Some(expected));
        for _ in 0..3 {
            assert_eq!(meter.next(), None);
            assert_eq!(meter.samples.sample_count, count as u64);
            assert_eq!(*shared.lock().unwrap(), Some(expected));
        }
    }
}

#[test]
fn all_source_metadata_and_interleaved_samples_pass_through_without_normalization() {
    for channels in [1, 2, 6] {
        for rate in [8_000, 44_100, 96_000] {
            let input: Vec<_> = [0.0, -0.0, 0.25, -0.5, 1.5, -2.0]
                .into_iter()
                .cycle()
                .take(8196)
                .collect();
            let mut reference = source(input.clone(), channels, rate);
            let shared = Loudness::default();
            let mut meter = LoudnessMeter::new(source(input, channels, rate), shared.clone());
            let mut count = 0;
            loop {
                assert_eq!(meter.current_span_len(), reference.current_span_len());
                assert_eq!(meter.channels(), reference.channels());
                assert_eq!(meter.sample_rate(), reference.sample_rate());
                assert_eq!(meter.total_duration(), reference.total_duration());
                let expected = reference.next();
                assert_eq!(meter.next().map(f32::to_bits), expected.map(f32::to_bits));
                if expected.is_none() {
                    break;
                }
                count += 1;
                assert_eq!(shared.lock().unwrap().is_some(), count >= 4096);
            }
            assert_eq!(meter.samples.sample_count, 8196);
        }
    }
}

#[test]
fn poisoned_shared_slot_does_not_interrupt_samples_or_accumulation() {
    let shared = Loudness::default();
    let poison = shared.clone();
    assert!(std::thread::spawn(move || {
        let _guard = poison.lock().unwrap();
        panic!("injected shared-slot failure");
    })
    .join()
    .is_err());
    let expected: Vec<_> = [0.25, -0.5].into_iter().cycle().take(8192).collect();
    let mut meter = LoudnessMeter::new(source(expected.clone(), 2, 48_000), shared.clone());
    assert_eq!(meter.by_ref().collect::<Vec<_>>(), expected);
    assert_eq!(meter.samples.sample_count, 8192);
    assert_eq!(meter.samples.peak, 0.5);
    assert!(shared.is_poisoned());
}

#[test]
fn accumulator_matches_the_previous_inline_policy_including_nonfinite_sample_behavior() {
    for values in [
        vec![0.0, 0.25, -0.5, 1.0, -1.0, 1e-6],
        vec![0.25, f32::NAN, -0.5],
        vec![f32::INFINITY, 0.0, f32::NEG_INFINITY],
    ] {
        let mut samples = SampleLoudness::default();
        let mut sum_squares = 0.0_f64;
        let mut sample_count = 0_u64;
        let mut peak = 0.0_f32;
        for sample in values.into_iter().cycle().take(16_385) {
            let value = f64::from(sample);
            sum_squares += value * value;
            sample_count += 1;
            peak = peak.max(sample.abs());
            let expected = if sample_count & 4095 == 0 {
                let rms = (sum_squares / sample_count as f64).sqrt();
                let db = if rms > 1e-6 {
                    20.0 * rms.log10()
                } else {
                    -100.0
                };
                Some((db as f32, peak))
            } else {
                None
            };
            let bits = |reading: (f32, f32)| (reading.0.to_bits(), reading.1.to_bits());
            assert_eq!(samples.observe(sample).map(bits), expected.map(bits));
        }
    }
}
