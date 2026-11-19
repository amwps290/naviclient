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
fn observing_audio_preserves_every_sample_and_source_metadata() {
    for enabled in [false, true] {
        let shared = Arc::new(AudioVisualizer::default());
        shared.set_enabled(enabled);
        let samples = [0.0, -0.0, 0.25, -1.25, f32::NAN, f32::INFINITY]
            .into_iter()
            .cycle()
            .take(8192)
            .collect::<Vec<_>>();
        let original = source(samples.clone(), 2, 48_000);
        let mut observer = WaveformSource::new(original.clone(), shared.clone());
        assert_eq!(observer.channels(), original.channels());
        assert_eq!(observer.sample_rate(), original.sample_rate());
        assert_eq!(observer.total_duration(), original.total_duration());
        assert_eq!(observer.current_span_len(), original.current_span_len());
        assert_eq!(observer.size_hint(), original.size_hint());
        assert_eq!(
            observer.by_ref().map(f32::to_bits).collect::<Vec<_>>(),
            samples.into_iter().map(f32::to_bits).collect::<Vec<_>>()
        );
        assert!(shared.snapshot().iter().all(|value| value.is_finite()));
    }
}

#[test]
fn envelopes_follow_actual_amplitude_including_opposite_phase_stereo() {
    for (channels, rate) in [(1, 8_000), (2, 44_100), (2, 48_000), (6, 96_000)] {
        let shared = Arc::new(AudioVisualizer::default());
        shared.set_enabled(true);
        let samples = [0.75, -0.75].into_iter().cycle().take(48_000).collect();
        let mut observer = WaveformSource::new(source(samples, channels, rate), shared.clone());
        let frame_samples =
            rate as usize * usize::from(channels) / (30 * WAVEFORM_BINS) * WAVEFORM_BINS;
        for _ in 0..frame_samples {
            observer.next().unwrap();
        }
        assert_eq!(shared.snapshot(), [0.75; WAVEFORM_BINS]);
    }
}

#[test]
fn disabling_and_reenabling_analysis_drops_the_previous_envelope() {
    let shared = Arc::new(AudioVisualizer::default());
    shared.set_enabled(true);
    let mut observer = WaveformSource::new(source(vec![0.5; 8192], 2, 48_000), shared.clone());
    for _ in 0..3200 {
        observer.next().unwrap();
    }
    assert_eq!(shared.snapshot(), [0.5; WAVEFORM_BINS]);
    shared.set_enabled(false);
    assert_eq!(shared.snapshot(), [0.0; WAVEFORM_BINS]);
    for _ in 0..100 {
        observer.next().unwrap();
    }
    shared.set_enabled(true);
    assert_eq!(shared.snapshot(), [0.0; WAVEFORM_BINS]);
    for _ in 0..3200 {
        observer.next().unwrap();
    }
    assert_eq!(shared.snapshot(), [0.5; WAVEFORM_BINS]);
}

#[test]
fn silence_has_a_flat_envelope_and_seek_keeps_the_wrapped_source_seekable() {
    let shared = Arc::new(AudioVisualizer::default());
    shared.set_enabled(true);
    let mut observer = WaveformSource::new(source(vec![0.0; 48_000], 1, 48_000), shared.clone());
    for _ in 0..3200 {
        assert_eq!(observer.next(), Some(0.0));
    }
    assert_eq!(shared.snapshot(), [0.0; WAVEFORM_BINS]);
    observer.try_seek(Duration::from_millis(500)).unwrap();
    assert_eq!(shared.snapshot(), [0.0; WAVEFORM_BINS]);
    assert_eq!(observer.next(), Some(0.0));
}
