//! Validated HTTP audio responses and byte-range metadata.

use std::io::{self, Read};

use anyhow::{anyhow, Result};
use reqwest::blocking::{Client, Response};
use reqwest::header::{HeaderMap, CONTENT_RANGE, CONTENT_TYPE, RANGE};
use reqwest::StatusCode;
use rust_i18n::t;
use url::Url;

use super::error::{classify_reqwest, failure_io, FailureError, StreamFailure};

#[derive(Debug, PartialEq, Eq)]
struct ByteRange {
    start: u64,
    total: Option<u64>,
    count: u64,
}

impl ByteRange {
    fn parse(value: &str) -> Option<Self> {
        fn number(value: &str) -> Option<u64> {
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            value.parse().ok()
        }

        let mut fields = value.split_whitespace();
        if !fields.next()?.eq_ignore_ascii_case("bytes") {
            return None;
        }
        let range = fields.next()?;
        if fields.next().is_some() {
            return None;
        }
        let (bounds, total) = range.split_once('/')?;
        let (start, end) = bounds.split_once('-')?;
        let start = number(start)?;
        let end = number(end)?;
        let count = end.checked_sub(start)?.checked_add(1)?;
        let total = if total == "*" {
            None
        } else {
            Some(number(total)?)
        };
        if total.is_some_and(|total| end >= total) {
            return None;
        }
        Some(Self {
            start,
            total,
            count,
        })
    }
}

#[derive(Debug)]
struct ResponseMetadata {
    total_bytes: Option<u64>,
    range_bytes: Option<u64>,
    mime_type: Option<String>,
}

impl ResponseMetadata {
    fn inspect(
        status: StatusCode,
        headers: &HeaderMap,
        payload_len: Option<u64>,
        position: u64,
    ) -> Result<Self> {
        let range = match status {
            StatusCode::OK if position == 0 => None,
            StatusCode::OK => {
                return Err(anyhow!(FailureError::new(
                    StreamFailure::Truncated,
                    "audio server ignored the requested byte range",
                )));
            }
            StatusCode::PARTIAL_CONTENT => {
                let range = headers
                    .get(CONTENT_RANGE)
                    .and_then(|value| value.to_str().ok())
                    .and_then(ByteRange::parse)
                    .ok_or_else(|| {
                        anyhow!(FailureError::new(
                            StreamFailure::Truncated,
                            "audio server returned an invalid Content-Range header",
                        ))
                    })?;
                if range.start != position {
                    return Err(anyhow!(FailureError::new(
                        StreamFailure::Truncated,
                        format!(
                            "audio server returned byte range starting at {} instead of {position}",
                            range.start
                        ),
                    )));
                }
                if let Some(len) = payload_len.filter(|len| *len != range.count) {
                    return Err(anyhow!(FailureError::new(
                        StreamFailure::Truncated,
                        format!(
                            "audio byte range contains {} bytes but Content-Length is {len}",
                            range.count,
                        ),
                    )));
                }
                Some(range)
            }
            _ => {
                return Err(anyhow!(FailureError::new(
                    StreamFailure::Http(status.as_u16()),
                    t!("audio request returned HTTP %{status}", status = status),
                )));
            }
        };
        let total_bytes = match range.as_ref() {
            Some(range) => range.total,
            None => payload_len,
        };
        Ok(Self {
            total_bytes,
            range_bytes: range.map(|range| range.count),
            mime_type: headers
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(str::to_owned),
        })
    }
}

pub(super) struct StreamResponse {
    response: Response,
    metadata: ResponseMetadata,
    remaining: Option<u64>,
}

impl StreamResponse {
    pub(super) fn status(&self) -> StatusCode {
        self.response.status()
    }

    pub(super) fn len(&self) -> Option<u64> {
        self.metadata.total_bytes
    }

    pub(super) fn range_supported(&self) -> bool {
        self.metadata.range_bytes.is_some()
    }

    pub(super) fn mime_type(&self) -> Option<&str> {
        self.metadata.mime_type.as_deref()
    }

    pub(super) fn validate_length(&self, expected: Option<u64>) -> Result<()> {
        if let (Some(expected), Some(actual)) = (expected, self.len()) {
            if expected != actual {
                return Err(anyhow!(FailureError::new(
                    StreamFailure::Truncated,
                    format!(
                        "audio stream length changed from {expected} to {actual} while resuming"
                    ),
                )));
            }
        }
        Ok(())
    }
}

impl Read for StreamResponse {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let limit = self.remaining.unwrap_or(u64::MAX).min(buffer.len() as u64) as usize;
        if limit == 0 {
            return Ok(0);
        }
        let read = self.response.read(&mut buffer[..limit])?;
        if read == 0 && self.remaining.is_some_and(|remaining| remaining > 0) {
            return Err(failure_io(
                StreamFailure::Truncated,
                "audio response ended before its declared byte range was complete",
            ));
        }
        if let Some(remaining) = self.remaining.as_mut() {
            *remaining -= read as u64;
        }
        Ok(read)
    }
}

pub(super) fn request_range(client: &Client, url: &str, position: u64) -> Result<StreamResponse> {
    let response = client
        .get(url)
        .header(RANGE, format!("bytes={position}-"))
        .send()
        .map_err(|error| {
            let kind = classify_reqwest(&error);
            let error = error.without_url();
            anyhow!(FailureError::new(
                kind,
                t!(
                    "requesting the audio stream failed: %{error}",
                    error = error
                ),
            ))
        })?;
    log::debug!(
        "audio HTTP response; endpoint={} range_start={position} status={} content_length={:?} content_range={:?} content_type={:?}",
        stream_endpoint(url), response.status(), response.content_length(),
        response.headers().get(CONTENT_RANGE).and_then(|value| value.to_str().ok()),
        response.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok())
    );
    let status = response.status();
    if !status.is_success() {
        return Err(anyhow!(FailureError::new(
            StreamFailure::Http(status.as_u16()),
            t!("audio request returned HTTP %{status}", status = status),
        )));
    }
    if response_is_error_envelope(&response) {
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let mut bytes = Vec::new();
        let _ = response.take(8 * 1024).read_to_end(&mut bytes);
        // A bounded read can end in the middle of a UTF-8 character. Retain the
        // diagnostic prefix, including bytes received before a transport error.
        let body = String::from_utf8_lossy(&bytes);
        let detail = subsonic_error_detail(&body);
        log::warn!(
            "stream endpoint returned a non-audio response; endpoint={} status={status} content_type={} body={}",
            stream_endpoint(url), content_type, body.trim()
        );
        return Err(anyhow!(FailureError::new(
            StreamFailure::Http(status.as_u16()),
            t!(
                "server refused the stream request: %{detail}",
                detail = detail
            ),
        )));
    }
    let metadata = ResponseMetadata::inspect(
        status,
        response.headers(),
        response.content_length(),
        position,
    )?;
    let remaining = metadata.range_bytes;
    Ok(StreamResponse {
        response,
        metadata,
        remaining,
    })
}

pub(super) fn stream_endpoint(url: &str) -> String {
    Url::parse(url)
        .ok()
        .map(|url| {
            let host = url.host_str().unwrap_or("unknown");
            let port = url
                .port()
                .map(|port| format!(":{port}"))
                .unwrap_or_default();
            format!("{}://{host}{port}{}", url.scheme(), url.path())
        })
        .unwrap_or_else(|| "invalid-stream-url".to_string())
}

pub(super) fn is_transcoded_stream(url: &str) -> bool {
    Url::parse(url).is_ok_and(|url| {
        url.query_pairs()
            .any(|(key, value)| key == "maxBitRate" && value != "0")
    })
}

fn response_is_error_envelope(response: &Response) -> bool {
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    content_type.starts_with("application/json")
        || content_type.starts_with("text/")
        || content_type.starts_with("application/xml")
}

fn subsonic_error_detail(body: &str) -> String {
    let trimmed = body.trim();
    let raw = || {
        if trimmed.is_empty() {
            t!("the server returned an empty response").to_string()
        } else {
            let prefix: String = trimmed.chars().take(200).collect();
            if prefix.chars().count() < trimmed.chars().count() {
                format!("{prefix}…")
            } else {
                prefix
            }
        }
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return raw();
    };
    let error = value
        .get("subsonic-response")
        .and_then(|response| response.get("error"))
        .or_else(|| value.get("error"));
    let Some(error) = error else { return raw() };
    let code = error.get("code").and_then(serde_json::Value::as_i64);
    let message = error
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    match (code, message.is_empty()) {
        (Some(code), false) => t!(
            "%{message} (Subsonic error %{code})",
            message = message,
            code = code
        )
        .to_string(),
        (Some(code), true) => t!("Subsonic error %{code}", code = code).to_string(),
        (None, false) => message.to_string(),
        (None, true) => raw(),
    }
}

#[cfg(test)]
mod tests;
