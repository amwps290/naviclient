use super::*;
use std::time::Duration;

fn device(
    name: Option<&str>,
    channels: u16,
    rate: u32,
    sample: f32,
) -> (DevicePlayback, rodio::queue::SourcesQueueOutput) {
    let channels = ChannelCount::new(channels).unwrap();
    let rate = SampleRate::new(rate).unwrap();
    let (player, source) = Player::new();
    player.append(rodio::buffer::SamplesBuffer::new(
        channels,
        rate,
        vec![sample; 4096],
    ));
    (
        DevicePlayback::headless(player, name.map(str::to_string), channels, rate),
        source,
    )
}

fn playback() -> PlaybackState {
    PlaybackState {
        active: true,
        output_connected: true,
        paused: true,
        position: Duration::from_secs(12),
        duration: Some(Duration::from_secs(180)),
        buffered: Duration::from_secs(42),
        transition_id: Some(7),
        seek_serial: 3,
        command_serial: 5,
        loudness_db: -14.0,
        loudness_peak: 0.8,
        output_name: Some("old output".into()),
        output_sample_rate: 48_000,
        output_channels: 2,
        ..Default::default()
    }
}

#[test]
fn failed_switch_preserves_output_samples_format_and_all_diagnostics() {
    let original = playback();
    let state = Arc::new(Mutex::new(original.clone()));
    let (old, mut source) = device(Some("old output"), 2, 48_000, 0.25);
    let mut output = AudioOutput::new(old);
    let eq = output.eq.clone();
    let handoff = output.handoff.clone();
    let mut opens = 0;

    let error = output
        .switch_with(&state, || {
            opens += 1;
            assert_eq!(source.next(), Some(0.25));
            assert_eq!(*state.lock().unwrap(), original);
            Err(anyhow::anyhow!("device disconnected").context("opening selected output"))
        })
        .unwrap_err();

    assert_eq!(opens, 1);
    assert_eq!(
        format!("{error:#}"),
        "opening selected output: device disconnected"
    );
    assert_eq!(*state.lock().unwrap(), original);
    assert_eq!(output.device.name.as_deref(), Some("old output"));
    assert_eq!(output.device.channels.get(), 2);
    assert_eq!(output.device.sample_rate.get(), 48_000);
    assert_eq!(source.next(), Some(0.25));
    assert!(output.eq.shares_state(&eq));
    assert!(Arc::ptr_eq(&output.handoff, &handoff));
}

#[test]
fn replacement_keeps_old_audio_live_until_new_player_and_format_are_ready() {
    let original = playback();
    let state = Arc::new(Mutex::new(original.clone()));
    let (old, mut old_source) = device(Some("old output"), 2, 48_000, 0.25);
    let mut output = AudioOutput::new(old);
    let eq = output.eq.clone();
    let handoff = output.handoff.clone();
    let settings = EqSettings {
        enabled: true,
        gains_db: [3.0; 10],
    };
    eq.set(settings.clone());
    eq.set_version_for_test(19);
    let (replacement, mut new_source) = device(Some("USB DAC"), 1, 96_000, 0.75);
    let mut opens = 0;

    output
        .switch_with(&state, || {
            opens += 1;
            assert_eq!(old_source.next(), Some(0.25));
            assert_eq!(*state.lock().unwrap(), original);
            Ok(replacement)
        })
        .unwrap();

    assert_eq!(opens, 1);
    assert_eq!(output.device.name.as_deref(), Some("USB DAC"));
    assert_eq!(output.device.channels.get(), 1);
    assert_eq!(output.device.sample_rate.get(), 96_000);
    assert_eq!(new_source.next(), Some(0.75));
    assert!(!output.device.player.empty());
    assert!(output.eq.shares_state(&eq));
    assert!(Arc::ptr_eq(&output.handoff, &handoff));
    assert_eq!(eq.snapshot(), (19, settings));
    let expected = PlaybackState {
        output_name: Some("USB DAC".into()),
        output_channels: 1,
        output_sample_rate: 96_000,
        ..original
    };
    assert_eq!(*state.lock().unwrap(), expected);
}

#[test]
fn default_device_fallback_reports_actual_format_without_a_stale_name() {
    let original = playback();
    let state = Arc::new(Mutex::new(original.clone()));
    let (old, _source) = device(Some("old output"), 2, 48_000, 0.25);
    let mut output = AudioOutput::new(old);
    let (fallback, _new_source) = device(None, 2, 44_100, 0.5);

    output.switch_with(&state, || Ok(fallback)).unwrap();

    assert_eq!(output.device.name, None);
    assert_eq!(
        *state.lock().unwrap(),
        PlaybackState {
            output_name: None,
            output_sample_rate: 44_100,
            ..original
        }
    );
}

#[test]
fn failed_replacement_after_success_keeps_the_latest_device() {
    let state = Arc::new(Mutex::new(playback()));
    let (old, _source) = device(Some("old output"), 2, 48_000, 0.25);
    let mut output = AudioOutput::new(old);
    let (replacement, mut source) = device(Some("USB DAC"), 2, 96_000, 0.75);
    output.switch_with(&state, || Ok(replacement)).unwrap();
    let previous = state.lock().unwrap().clone();

    assert!(output
        .switch_with(&state, || Err(anyhow::anyhow!("second output unavailable")))
        .is_err());

    assert_eq!(*state.lock().unwrap(), previous);
    assert_eq!(output.device.name.as_deref(), Some("USB DAC"));
    assert_eq!(output.device.sample_rate.get(), 96_000);
    assert_eq!(source.next(), Some(0.75));
}
