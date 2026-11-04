//! Audio decoding, startup buffering, and decoded-stream stall handling.

use super::*;

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
    let buffer_progress = Arc::clone(&reader.buffer_progress);
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
        anyhow!("unsupported audio format: {error}")
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
            .map_err(|error| anyhow!("audio seek failed: {error}"))?;
    }
    let remaining_duration = duration.map(|duration| duration.saturating_sub(start_position));
    let (source, stream_error) = BufferedStreamSource::new(decoder, remaining_duration, control)?;
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

pub(super) enum StreamMessage {
    Samples(Vec<Sample>),
    End,
    Error(FailureError),
}

pub(super) struct BufferedStreamSource {
    pub(super) control: Arc<StreamControl>,
    pub(super) receiver: Receiver<StreamMessage>,
    pub(super) queued: VecDeque<Vec<Sample>>,
    pub(super) current: std::vec::IntoIter<Sample>,
    pub(super) channels: ChannelCount,
    pub(super) sample_rate: SampleRate,
    pub(super) duration: Option<Duration>,
    pub(super) producer_finished: bool,
    pub(super) terminal_error: Option<FailureError>,
    pub(super) shared_error: SharedStreamError,
    pub(super) last_data: Instant,
    pub(super) silence_remaining: usize,
}

impl BufferedStreamSource {
    fn new(
        decoder: Decoder<BufReader<HttpRangeReader>>,
        duration: Option<Duration>,
        control: Arc<StreamControl>,
    ) -> Result<(Self, SharedStreamError)> {
        let channels = decoder.channels();
        let sample_rate = decoder.sample_rate();
        let (sender, receiver) = mpsc::sync_channel(DECODED_BUFFER_CHUNKS);
        let shared_error = Arc::new(Mutex::new(None));

        let decoder_control = control.clone();
        let decoder_guard = control.decoder_guard();
        thread::Builder::new()
            .name("audio-decoder".to_string())
            .spawn(move || {
                let _guard = decoder_guard;
                decode_stream(
                    decoder,
                    sender,
                    duration,
                    channels,
                    sample_rate,
                    decoder_control,
                )
            })
            .context("failed to start the audio decoder thread")?;

        let mut queued = VecDeque::new();
        let mut producer_finished = false;
        let mut terminal_error = None;
        let deadline = Instant::now() + STARTUP_BUFFER_TIMEOUT;

        while queued.len() < STARTUP_BUFFER_CHUNKS && !producer_finished {
            control.check()?;
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            match receiver.recv_timeout(remaining.min(Duration::from_millis(100))) {
                Ok(StreamMessage::Samples(samples)) => queued.push_back(samples),
                Ok(StreamMessage::End) => producer_finished = true,
                Ok(StreamMessage::Error(error)) => {
                    terminal_error = Some(error);
                    producer_finished = true;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    terminal_error = Some(FailureError::new(
                        StreamFailure::Decode,
                        "audio decoder stopped unexpectedly",
                    ));
                    producer_finished = true;
                }
            }
        }

        if queued.is_empty() {
            if let Some(error) = terminal_error {
                log::error!("audio decoder failed before startup buffer was ready: {error}");
                return Err(anyhow!(error));
            }
            log::error!(
                "audio startup buffer timed out; timeout_ms={}",
                STARTUP_BUFFER_TIMEOUT.as_millis()
            );
            return Err(anyhow!(FailureError::new(
                StreamFailure::NoData,
                t!(
                    "no audio arrived within %{seconds}s while buffering",
                    seconds = STARTUP_BUFFER_TIMEOUT.as_secs()
                )
                .to_string(),
            )));
        }
        log::info!(
            "audio startup buffer ready; chunks={} target_chunks={} producer_finished={producer_finished}",
            queued.len(),
            STARTUP_BUFFER_CHUNKS
        );

        let source = Self {
            control,
            receiver,
            queued,
            current: Vec::new().into_iter(),
            channels,
            sample_rate,
            duration,
            producer_finished,
            terminal_error,
            shared_error: Arc::clone(&shared_error),
            last_data: Instant::now(),
            silence_remaining: 0,
        };
        Ok((source, shared_error))
    }

    fn finish(&mut self) -> Option<Sample> {
        if let Some(error) = self.terminal_error.take() {
            if let Ok(mut shared_error) = self.shared_error.lock() {
                *shared_error = Some(error);
            }
        }
        None
    }

    fn fail(&mut self, error: FailureError) -> Option<Sample> {
        self.producer_finished = true;
        self.terminal_error = Some(error);
        self.finish()
    }
}

impl Drop for BufferedStreamSource {
    fn drop(&mut self) {
        self.control.cancel();
    }
}

impl Iterator for BufferedStreamSource {
    type Item = Sample;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(sample) = self.current.next() {
                return Some(sample);
            }
            if let Some(samples) = self.queued.pop_front() {
                self.last_data = Instant::now();
                self.control.mark_data();
                self.current = samples.into_iter();
                continue;
            }
            if self.producer_finished {
                return self.finish();
            }
            if self.silence_remaining > 0 {
                self.silence_remaining -= 1;
                return Some(0.0);
            }

            match self.receiver.try_recv() {
                Ok(StreamMessage::Samples(samples)) => {
                    self.last_data = Instant::now();
                    self.control.mark_data();
                    self.current = samples.into_iter();
                }
                Ok(StreamMessage::End) => self.producer_finished = true,
                Ok(StreamMessage::Error(error)) => {
                    // 解码器在 I/O 失败时会静默结束；reader 记下的原因更接近真相。
                    self.terminal_error = Some(self.control.take_io_error().unwrap_or(error));
                    self.producer_finished = true;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    if self.last_data.elapsed() >= STREAM_STALL_TIMEOUT {
                        let error = self.control.take_io_error().unwrap_or_else(|| {
                            FailureError::new(
                                StreamFailure::NoData,
                                t!(
                                    "no audio data for %{seconds}s while playback was running",
                                    seconds = STREAM_STALL_TIMEOUT.as_secs()
                                )
                                .to_string(),
                            )
                        });
                        return self.fail(error);
                    }
                    self.control.mark_synthetic_silence_frame(self.sample_rate);
                    self.silence_remaining = usize::from(self.channels.get()).saturating_sub(1);
                    return Some(0.0);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    return self.fail(FailureError::new(
                        StreamFailure::Decode,
                        "audio decoder stopped unexpectedly",
                    ));
                }
            }
        }
    }
}

impl Source for BufferedStreamSource {
    fn current_span_len(&self) -> Option<usize> {
        if self.terminal_error.is_none()
            && self.producer_finished
            && self.queued.is_empty()
            && self.current.len() == 0
        {
            Some(0)
        } else {
            None
        }
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.duration
    }
}

fn decode_stream(
    mut decoder: Decoder<BufReader<HttpRangeReader>>,
    sender: mpsc::SyncSender<StreamMessage>,
    duration: Option<Duration>,
    channels: ChannelCount,
    sample_rate: SampleRate,
    control: Arc<StreamControl>,
) {
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

    log::debug!(
        "audio decoder reached stream end; decoded_samples={decoded_samples} decoded_ms={}",
        decoded_duration(decoded_samples, channels, sample_rate).as_millis()
    );
    let message = if stream_ended_too_early(decoded_samples, channels, sample_rate, duration) {
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
        control.fully_decoded.store(true, Ordering::Release);
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
