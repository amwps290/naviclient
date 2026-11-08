//! Audio byte cursors, cache reuse, and retry handling over validated responses.

use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::blocking::Client;

use super::error::{classify_io, failure_io, FailureError, StreamFailure};
use super::stream_cache::{BufferProgress, StreamCache};
use super::stream_control::StreamControl;
use super::stream_response::{
    initial_resource_response, is_transcoded_stream, request_resource_range, stream_endpoint,
    StreamResponse,
};
use crate::source::HttpResource;

const HTTP_READ_RETRIES: usize = 3;
const TRANSCODE_END_RETRIES: usize = 1;

pub(super) fn near_estimated_end(position: u64, estimated_len: u64) -> bool {
    estimated_len > 0 && position.saturating_mul(100) >= estimated_len.saturating_mul(95)
}

pub(super) struct HttpRangeReader {
    control: Arc<StreamControl>,
    client: Client,
    url: HttpResource,
    response: Option<StreamResponse>,
    response_position: u64,
    position: u64,
    len: Option<u64>,
    range_supported: bool,
    decoder_seekable: bool,
    length_is_estimate: bool,
    mime_type: Option<String>,
    cache: StreamCache,
}

impl HttpRangeReader {
    #[cfg(test)]
    pub(super) fn open(
        client: Client,
        url: String,
        cache_dir: &Path,
        cache_key: &str,
    ) -> Result<Self> {
        Self::open_controlled(client, url, cache_dir, cache_key, StreamControl::new(false))
    }

    #[cfg(test)]
    pub(super) fn open_controlled(
        client: Client,
        url: String,
        cache_dir: &Path,
        cache_key: &str,
        control: Arc<StreamControl>,
    ) -> Result<Self> {
        Self::open_resource(client, url.into(), cache_dir, cache_key, control)
    }

    pub(super) fn open_resource(
        client: Client,
        url: HttpResource,
        cache_dir: &Path,
        cache_key: &str,
        control: Arc<StreamControl>,
    ) -> Result<Self> {
        control.check()?;
        let mut cache = StreamCache::open_with_policy(
            cache_dir,
            cache_key,
            control.clone(),
            url.cache_policy(),
        )?;
        if cache.complete() {
            let len = cache.cached_len();
            let mime_type = url.mime_hint().map(str::to_owned);
            return Ok(Self {
                control,
                client,
                url,
                response: None,
                response_position: 0,
                position: 0,
                len: Some(len),
                range_supported: true,
                decoder_seekable: true,
                length_is_estimate: false,
                mime_type,
                cache,
            });
        }

        let (client, url, response) =
            initial_resource_response(client, url).context("failed to request audio stream")?;
        control.check()?;
        let range_supported = response.range_supported();
        let len = response.len();
        let length_is_estimate = url.length_is_estimate() || is_transcoded_stream(&url);
        let decoder_seekable = range_supported && len.is_some() && !length_is_estimate;
        let mime_type = match response.mime_type() {
            None | Some("application/octet-stream") => url
                .mime_hint()
                .map(str::to_owned)
                .or_else(|| response.mime_type().map(str::to_owned)),
            mime => mime.map(str::to_owned),
        };
        cache.prepare_partial(len, range_supported);

        log::info!(
            "audio network stream opened; endpoint={} status={} bytes={len:?} range_supported={range_supported} decoder_seekable={decoder_seekable} mime={mime_type:?} cached_bytes={}",
            stream_endpoint(&url), response.status(), cache.cached_len()
        );
        Ok(Self {
            control,
            client,
            url,
            response: Some(response),
            response_position: 0,
            position: 0,
            len,
            range_supported,
            decoder_seekable,
            length_is_estimate,
            mime_type,
            cache,
        })
    }

    pub(super) fn buffer_progress(&self) -> Arc<BufferProgress> {
        self.cache.progress()
    }

    pub(super) fn len(&self) -> Option<u64> {
        self.len
    }

    pub(super) fn is_seekable(&self) -> bool {
        self.cache.complete() || self.decoder_seekable
    }

    pub(super) fn mime_type(&self) -> Option<&str> {
        self.mime_type.as_deref()
    }

    fn reopen_at(&mut self, position: u64) -> io::Result<()> {
        self.control.check()?;
        if position != 0 && !self.range_supported {
            return Err(failure_io(
                StreamFailure::Truncated,
                "audio server does not support HTTP range requests",
            ));
        }

        log::debug!(
            "reopening audio HTTP stream; endpoint={} byte_position={position}",
            stream_endpoint(&self.url)
        );
        let response = request_resource_range(&self.client, &self.url, position)
            .and_then(|response| {
                if !self.length_is_estimate {
                    response.validate_length(self.len)?;
                }
                Ok(response)
            })
            .map_err(|error| {
                let io = io_error(error);
                self.control
                    .record_io_error(FailureError::new(classify_io(&io), io.to_string()));
                io
            })?;
        self.range_supported |= response.range_supported();
        self.response = Some(response);
        self.response_position = position;
        Ok(())
    }

    fn finish_estimated_stream(&mut self) {
        let estimated_len = self.len;
        self.len = Some(self.position);
        log::info!(
            "transcoded stream reached actual end; actual_bytes={} estimated_bytes={estimated_len:?}",
            self.position
        );
        self.cache.finish_estimated_stream(self.position);
    }

    fn near_estimated_end(&self) -> bool {
        self.length_is_estimate
            && self
                .len
                .is_some_and(|len| near_estimated_end(self.position, len))
    }
}

impl Read for HttpRangeReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.control.check()?;
        if buffer.is_empty() {
            return Ok(0);
        }
        let cached = self
            .cache
            .read(self.position, buffer)
            .inspect_err(|error| {
                self.control
                    .record_io_error(FailureError::new(classify_io(error), error.to_string()));
            })?;
        if let Some(read) = cached {
            self.position = self.position.saturating_add(read as u64);
            return Ok(read);
        }
        if self.cache.complete() || self.len == Some(self.position) {
            return Ok(0);
        }
        if self.response_position != self.position {
            self.reopen_at(self.position)?;
        }

        for attempt in 0..=HTTP_READ_RETRIES {
            self.control.check()?;
            let start = self.position;
            let result = self
                .response
                .as_mut()
                .ok_or_else(|| {
                    failure_io(StreamFailure::Connect, "audio HTTP response is unavailable")
                })?
                .read(buffer);
            self.control.check()?;
            match result {
                Ok(0) if self.near_estimated_end() => {
                    self.finish_estimated_stream();
                    return Ok(0);
                }
                Ok(0) if self.len.is_some_and(|len| self.position < len) => {
                    if !self.range_supported || attempt == HTTP_READ_RETRIES {
                        let error = failure_io(
                            StreamFailure::Truncated,
                            format!(
                                "stream ended at byte {} of {}",
                                self.position,
                                self.len.unwrap_or_default()
                            ),
                        );
                        self.control.record_io_error(FailureError::new(
                            StreamFailure::Truncated,
                            error.to_string(),
                        ));
                        return Err(error);
                    }
                    thread::sleep(Duration::from_millis(250 * (attempt as u64 + 1)));
                    self.reopen_at(self.position)?;
                }
                Ok(read) => {
                    self.cache.append(start, &buffer[..read]);
                    self.position = self.position.saturating_add(read as u64);
                    self.response_position = self.position;
                    return Ok(read);
                }
                Err(error) => {
                    if self.near_estimated_end() && attempt >= TRANSCODE_END_RETRIES {
                        self.finish_estimated_stream();
                        return Ok(0);
                    }
                    if !self.range_supported || attempt == HTTP_READ_RETRIES {
                        let kind = classify_io(&error);
                        log::warn!(
                            "audio stream read failed permanently; byte={} kind={kind:?}: {error}",
                            self.position
                        );
                        self.control
                            .record_io_error(FailureError::new(kind, error.to_string()));
                        return Err(error);
                    }
                    if self.near_estimated_end() {
                        log::debug!(
                            "transcoded stream read ended near estimated length; byte={} retry={}: {error}",
                            self.position,
                            attempt + 1
                        );
                    } else {
                        log::warn!(
                            "audio stream read failed at byte {}, retrying: {error}",
                            self.position
                        );
                    }
                    thread::sleep(Duration::from_millis(250 * (attempt as u64 + 1)));
                    self.reopen_at(self.position)?;
                }
            }
        }
        unreachable!("audio stream retry loop always returns")
    }
}

impl Seek for HttpRangeReader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.control.check()?;
        self.position = seek_target(self.position, self.len, from)?;
        Ok(self.position)
    }
}

pub(super) fn seek_target(current: u64, len: Option<u64>, from: SeekFrom) -> io::Result<u64> {
    let target = match from {
        SeekFrom::Start(position) => i128::from(position),
        SeekFrom::Current(offset) => i128::from(current) + i128::from(offset),
        SeekFrom::End(offset) => {
            let len = len.ok_or_else(|| {
                io::Error::new(io::ErrorKind::Unsupported, "audio length is unknown")
            })?;
            i128::from(len) + i128::from(offset)
        }
    };
    if target < 0 || target > i128::from(u64::MAX) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid audio stream seek",
        ));
    }
    let target = target as u64;
    if len.is_some_and(|len| target > len) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "audio stream seek is past the end",
        ));
    }
    Ok(target)
}

/// 把输出设备时钟换算成真实曲目位置。网络停滞时注入的静音会推进
/// `Player::get_pos()`，但它没有消费歌曲内容，因此必须扣除。
pub(super) fn logical_playback_position(
    position_offset: Duration,
    output_position: Duration,
    synthetic_silence: Duration,
) -> Duration {
    position_offset.saturating_add(output_position.saturating_sub(synthetic_silence))
}

pub(super) fn io_error(error: anyhow::Error) -> io::Error {
    match error.downcast::<FailureError>() {
        Ok(failure) => failure_io(failure.kind, failure.detail),
        Err(other) => io::Error::other(other),
    }
}

#[cfg(test)]
pub(super) mod resource_tests;
#[cfg(test)]
pub(super) mod tests;
