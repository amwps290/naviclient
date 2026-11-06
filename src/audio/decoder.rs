//! Decoder construction, bounded PCM production, and terminal-result classification.

use std::io::{self, BufReader};
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{anyhow, Result};
use reqwest::blocking::Client;
use rodio::{ChannelCount, Decoder, Sample, SampleRate, Source};
use rust_i18n::t;

use super::buffered_stream::{BufferedStreamSource, StreamMessage};
use super::error::{FailureError, SharedStreamError, StreamFailure};
use super::format_playback;
use super::http_stream::HttpRangeReader;
use super::stream_cache::BufferProgress;
use super::stream_control::StreamControl;
use super::stream_response::stream_endpoint;

const STREAM_BUFFER_BYTES: usize = 1024 * 1024;
const DECODED_CHUNK_FRAMES: usize = 4096;
const DECODED_BUFFER_CHUNKS: usize = 512;

pub(super) fn play_url(
    client: Client,
    url: &str,
    cache_dir: &Path,
    cache_key: &str,
    duration_hint: Option<Duration>,
    start_position: Duration,
    control: Arc<StreamControl>,
) -> Result<PreparedStream> {
    log::debug!(
        "preparing audio stream; endpoint={} start_ms={}",
        stream_endpoint(url),
        start_position.as_millis()
    );
    let reader = HttpRangeReader::open_controlled(
        client,
        url.to_string(),
        cache_dir,
        cache_key,
        control.clone(),
    )?;
    let buffer_progress = reader.buffer_progress();
    let byte_len = reader.len();
    let seekable = reader.is_seekable();
    let mime_type = reader.mime_type().map(str::to_owned);
    log::debug!("audio reader ready; byte_len={byte_len:?} seekable={seekable} mime={mime_type:?}");
    let buffered = BufReader::with_capacity(STREAM_BUFFER_BYTES, reader);

    let mut builder = Decoder::builder()
        .with_data(buffered)
        .with_seekable(seekable)
        .with_gapless(true);
    if let Some(byte_len) = byte_len {
        builder = builder.with_byte_len(byte_len).with_seekable(seekable);
    }
    if let Some(mime_type) = mime_type.as_deref() {
        builder = builder.with_mime_type(mime_type);
    }

    let mut decoder = builder.build().map_err(|error| {
        log::error!(
            "audio decoder initialization failed; byte_len={byte_len:?} seekable={seekable} mime={mime_type:?}: {error}"
        );
        decoder_failure(&control, format!("unsupported audio format: {error}"))
    })?;
    let duration = decoder.total_duration().or(duration_hint);
    log::info!(
        "audio decoder ready; channels={} sample_rate={} duration_ms={:?}",
        decoder.channels().get(),
        decoder.sample_rate().get(),
        duration.map(|duration| duration.as_millis())
    );
    let start_position = duration
        .map(|duration| start_position.min(duration))
        .unwrap_or(start_position);
    if !start_position.is_zero() {
        decoder
            .try_seek(start_position)
            .map_err(|error| decoder_failure(&control, format!("audio seek failed: {error}")))?;
    }
    let remaining_duration = duration.map(|duration| duration.saturating_sub(start_position));
    let channels = decoder.channels();
    let sample_rate = decoder.sample_rate();
    let receiver = spawn_decoder(decoder, remaining_duration, control.clone())?;
    let (source, stream_error) =
        BufferedStreamSource::new(receiver, remaining_duration, channels, sample_rate, control)?;
    Ok(PreparedStream {
        source,
        duration,
        error: stream_error,
        buffer_progress,
    })
}

pub(super) struct PreparedStream {
    pub(super) source: BufferedStreamSource,
    pub(super) duration: Option<Duration>,
    pub(super) error: SharedStreamError,
    pub(super) buffer_progress: Arc<BufferProgress>,
}

fn decoder_failure(control: &StreamControl, fallback: String) -> anyhow::Error {
    match control.take_io_error() {
        Some(error) => anyhow!(error),
        None => anyhow!(fallback),
    }
}

fn spawn_decoder<D: Source<Item = Sample> + Send + 'static>(
    decoder: D,
    duration: Option<Duration>,
    control: Arc<StreamControl>,
) -> Result<Receiver<StreamMessage>> {
    spawn_with(decoder, duration, control, |task| {
        thread::Builder::new()
            .name("audio-decoder".to_string())
            .spawn(task)
            .map(|_| ())
    })
}

fn spawn_with<D: Source<Item = Sample> + Send + 'static>(
    decoder: D,
    duration: Option<Duration>,
    control: Arc<StreamControl>,
    spawn: impl FnOnce(Box<dyn FnOnce() + Send>) -> io::Result<()>,
) -> Result<Receiver<StreamMessage>> {
    let (sender, receiver) = mpsc::sync_channel(DECODED_BUFFER_CHUNKS);
    let decoder_control = control.clone();
    let guard = control.decoder_guard();
    let task = Box::new(move || {
        let _guard = guard;
        decode_stream(decoder, sender, duration, decoder_control);
    });
    if let Err(error) = spawn(task) {
        control.cancel();
        return Err(anyhow!(FailureError::new(
            StreamFailure::Preparation,
            format!("failed to start the audio decoder thread: {error}")
        )));
    }
    Ok(receiver)
}

fn decode_stream(
    mut decoder: impl Source<Item = Sample>,
    sender: mpsc::SyncSender<StreamMessage>,
    duration: Option<Duration>,
    control: Arc<StreamControl>,
) {
    let channels = decoder.channels();
    let sample_rate = decoder.sample_rate();
    let samples_per_chunk = DECODED_CHUNK_FRAMES * usize::from(channels.get());
    let mut decoded_samples = 0_u64;
    log::debug!(
        "audio decoder thread running; channels={} sample_rate={} chunk_samples={samples_per_chunk}",
        channels.get(),
        sample_rate.get()
    );

    let mut chunks = 0;
    while control.wait_for_decode(chunks) {
        let samples: Vec<_> = decoder.by_ref().take(samples_per_chunk).collect();
        if samples.is_empty() {
            break;
        }
        decoded_samples = decoded_samples.saturating_add(samples.len() as u64);
        if sender.send(StreamMessage::Samples(samples)).is_err() {
            log::debug!("audio decoder stopped because playback source was dropped");
            return;
        }
        chunks += 1;
    }

    if control.check().is_err() {
        log::debug!("audio decoder stopped because preparation was cancelled");
        return;
    }

    log::debug!(
        "audio decoder reached stream end; decoded_samples={decoded_samples} decoded_ms={}",
        decoded_duration(decoded_samples, channels, sample_rate).as_millis()
    );
    let message = if let Some(error) = control.take_io_error() {
        StreamMessage::Error(error)
    } else if decoded_samples == 0 {
        StreamMessage::Error(FailureError::new(
            StreamFailure::Decode,
            "audio stream contains no playable samples",
        ))
    } else if stream_ended_too_early(decoded_samples, channels, sample_rate, duration) {
        let decoded_duration = decoded_duration(decoded_samples, channels, sample_rate);
        StreamMessage::Error(FailureError::new(
            StreamFailure::Truncated,
            t!(
                "stream ended at %{actual}, expected about %{expected}",
                actual = format_playback(decoded_duration),
                expected = format_playback(duration.unwrap_or_default())
            )
            .to_string(),
        ))
    } else {
        StreamMessage::End
    };
    if matches!(message, StreamMessage::End) && control.check().is_ok() {
        control.mark_fully_decoded();
    }
    let _ = sender.send(message);
}

pub(super) fn decoded_duration(
    decoded_samples: u64,
    channels: ChannelCount,
    sample_rate: SampleRate,
) -> Duration {
    let samples_per_second = u64::from(channels.get()) * u64::from(sample_rate.get());
    Duration::from_secs_f64(decoded_samples as f64 / samples_per_second as f64)
}

pub(super) fn stream_ended_too_early(
    decoded_samples: u64,
    channels: ChannelCount,
    sample_rate: SampleRate,
    expected: Option<Duration>,
) -> bool {
    let Some(expected) = expected.filter(|duration| !duration.is_zero()) else {
        return false;
    };
    let actual = decoded_duration(decoded_samples, channels, sample_rate);
    let tolerance = Duration::from_secs_f64((expected.as_secs_f64() * 0.02).max(5.0));
    actual.saturating_add(tolerance) < expected
}

#[cfg(test)]
mod tests;
