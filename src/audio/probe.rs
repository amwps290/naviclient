//! Headless verification through the real reader and asynchronous PCM decoder.

use super::{decoder::play_resource, stream_control::StreamControl};
use crate::source::PlaybackResource;
use anyhow::{bail, Result};
use rodio::Source;
use std::time::Duration;

pub(crate) struct DecodeEvidence {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: usize,
    pub seeks: usize,
}

pub(crate) fn decode(resource: &PlaybackResource) -> Result<DecodeEvidence> {
    let cache = tempfile::tempdir()?;
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(crate::source::resource::timing::STREAM_TIMEOUT)
        .build()
        .map_err(reqwest::Error::without_url)?;
    let mut evidence = DecodeEvidence {
        sample_rate: 0,
        channels: 0,
        samples: 0,
        seeks: 0,
    };
    let duration = resource.duration.unwrap_or(Duration::from_secs(10));
    for position in [
        Duration::ZERO,
        duration / 2,
        Duration::from_secs(1).min(duration / 4),
    ] {
        let control = StreamControl::new(false);
        let result = (|| {
            let stream = play_resource(
                client.clone(),
                resource.http.clone().with_backups(resource.backups.clone()),
                cache.path(),
                &resource.cache_key(),
                resource.duration,
                position,
                control.clone(),
            )?;
            evidence.sample_rate = stream.source.sample_rate().get();
            evidence.channels = stream.source.channels().get();
            let target = evidence.sample_rate as usize * evidence.channels as usize;
            let error = stream.error.clone();
            let mut source = stream.source;
            let mut count = 0;
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            // The consumer injects silence while its decoder waits for bytes.
            // Count actual PCM frames, so a stalled stream cannot pass this probe.
            while count < target {
                if std::time::Instant::now() >= deadline {
                    bail!("audio decoder timed out waiting for PCM");
                }
                let silence = control.synthetic_silence_duration();
                let mut frame = 0;
                for _ in 0..evidence.channels {
                    let Some(sample) = source.next() else { break };
                    if !sample.is_finite() {
                        bail!("audio decoder produced a non-finite PCM sample");
                    }
                    frame += 1;
                }
                if frame != evidence.channels as usize {
                    break;
                }
                if control.synthetic_silence_duration() == silence {
                    count += frame;
                }
                if error.lock().unwrap().is_some() {
                    break;
                }
            }
            if let Some(error) = error.lock().unwrap().as_ref() {
                bail!("audio decoding failed: {error}");
            }
            if count != target {
                bail!("audio decoder did not produce one second of PCM");
            }
            evidence.samples += count;
            if !position.is_zero() {
                evidence.seeks += 1;
            }
            Ok(())
        })();
        control.cancel();
        result?;
    }
    Ok(evidence)
}
