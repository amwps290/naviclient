//! Startup-buffer ownership, nonblocking playback consumption, and stall handling.

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use rodio::{ChannelCount, Sample, SampleRate, Source};
use rust_i18n::t;

use super::error::{FailureError, SharedStreamError, StreamFailure};
use super::stream_control::StreamControl;

pub(super) const STARTUP_BUFFER_CHUNKS: usize = 16;
const STARTUP_BUFFER_TIMEOUT: Duration = Duration::from_secs(20);
const STREAM_STALL_TIMEOUT: Duration = Duration::from_secs(20);

pub(super) enum StreamMessage {
    Samples(Vec<Sample>),
    End,
    Error(FailureError),
}

pub(super) struct BufferedStreamSource {
    control: Arc<StreamControl>,
    receiver: Receiver<StreamMessage>,
    queued: VecDeque<Vec<Sample>>,
    current: std::vec::IntoIter<Sample>,
    channels: ChannelCount,
    sample_rate: SampleRate,
    duration: Option<Duration>,
    producer_finished: bool,
    terminal_error: Option<FailureError>,
    shared_error: SharedStreamError,
    last_data: Instant,
    silence_remaining: usize,
}

impl BufferedStreamSource {
    pub(super) fn new(
        receiver: Receiver<StreamMessage>,
        duration: Option<Duration>,
        channels: ChannelCount,
        sample_rate: SampleRate,
        control: Arc<StreamControl>,
    ) -> Result<(Self, SharedStreamError)> {
        Self::with_timeout(
            receiver,
            duration,
            channels,
            sample_rate,
            control,
            STARTUP_BUFFER_TIMEOUT,
        )
    }

    fn with_timeout(
        receiver: Receiver<StreamMessage>,
        duration: Option<Duration>,
        channels: ChannelCount,
        sample_rate: SampleRate,
        control: Arc<StreamControl>,
        timeout: Duration,
    ) -> Result<(Self, SharedStreamError)> {
        let mut source = Self {
            control,
            receiver,
            queued: VecDeque::new(),
            current: Vec::new().into_iter(),
            channels,
            sample_rate,
            duration,
            producer_finished: false,
            terminal_error: None,
            shared_error: Arc::new(Mutex::new(None)),
            last_data: Instant::now(),
            silence_remaining: 0,
        };
        // Construct the owner before waiting: any startup failure drops the
        // receiver and cancels the decoder, including speculative sleepers.
        source.fill_startup(timeout)?;
        let error = source.shared_error.clone();
        Ok((source, error))
    }

    fn fill_startup(&mut self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        while self.queued.len() < STARTUP_BUFFER_CHUNKS && !self.producer_finished {
            self.control.check()?;
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            match self
                .receiver
                .recv_timeout(remaining.min(Duration::from_millis(100)))
            {
                Ok(StreamMessage::Samples(samples)) => self.queued.push_back(samples),
                Ok(StreamMessage::End) => self.producer_finished = true,
                Ok(StreamMessage::Error(error)) => {
                    self.terminal_error = Some(error);
                    self.producer_finished = true;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.terminal_error = Some(FailureError::new(
                        StreamFailure::Decode,
                        "audio decoder stopped unexpectedly",
                    ));
                    self.producer_finished = true;
                }
            }
        }
        if self.queued.is_empty() {
            let error = self
                .control
                .take_io_error()
                .or_else(|| self.terminal_error.take())
                .unwrap_or_else(|| {
                    if self.producer_finished {
                        FailureError::new(
                            StreamFailure::Decode,
                            "audio stream contains no playable samples",
                        )
                    } else {
                        FailureError::new(
                            StreamFailure::NoData,
                            t!(
                                "no audio arrived within %{seconds}s while buffering",
                                seconds = timeout.as_secs()
                            )
                            .to_string(),
                        )
                    }
                });
            log::error!("audio decoder failed before startup buffer was ready: {error}");
            return Err(anyhow!(error));
        }
        self.last_data = Instant::now();
        log::info!(
            "audio startup buffer ready; chunks={} target_chunks={} producer_finished={}",
            self.queued.len(),
            STARTUP_BUFFER_CHUNKS,
            self.producer_finished
        );
        Ok(())
    }

    pub(super) fn control(&self) -> &Arc<StreamControl> {
        &self.control
    }

    pub(super) fn startup_error(&self) -> Option<&FailureError> {
        self.terminal_error.as_ref()
    }

    pub(super) fn promote(&mut self) {
        self.last_data = Instant::now();
        self.control.promote();
    }

    fn finish(&mut self) -> Option<Sample> {
        if let Some(error) = self.control.take_io_error().or(self.terminal_error.take()) {
            if let Ok(mut shared_error) = self.shared_error.lock() {
                if shared_error.is_none() {
                    *shared_error = Some(error);
                }
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

#[cfg(test)]
pub(super) mod tests;
