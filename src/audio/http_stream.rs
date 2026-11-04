//! HTTP Range 读取、磁盘缓存、断点续传和流错误解析。

use super::*;

pub(super) fn prepare_cache_dir(cache_dir: &Path) -> Result<()> {
    fs::create_dir_all(cache_dir).context("failed to create audio cache directory")?;

    let mut entries = fs::read_dir(cache_dir)
        .context("failed to read audio cache directory")?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            metadata.is_file().then(|| {
                (
                    entry.path(),
                    metadata.len(),
                    metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                )
            })
        })
        .collect::<Vec<_>>();
    let mut total = entries.iter().map(|(_, len, _)| len).sum::<u64>();
    if total <= MAX_AUDIO_CACHE_BYTES {
        return Ok(());
    }

    entries.sort_by_key(|(_, _, modified)| *modified);
    for (path, len, _) in entries {
        if total <= MAX_AUDIO_CACHE_BYTES {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(len);
        }
    }
    Ok(())
}

pub(super) fn cache_paths(cache_dir: &Path, cache_key: &str) -> (PathBuf, PathBuf) {
    let mut hasher = Md5::new();
    hasher.update(cache_key.as_bytes());
    let name = format!("{:x}", hasher.finalize());
    (
        cache_dir.join(format!("{name}.audio")),
        cache_dir.join(format!("{name}.part")),
    )
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

pub(super) struct BufferProgress {
    pub(super) cached_bytes: AtomicU64,
    pub(super) total_bytes: AtomicU64,
}

impl BufferProgress {
    pub(super) fn new(cached_bytes: u64, total_bytes: Option<u64>) -> Self {
        Self {
            cached_bytes: AtomicU64::new(cached_bytes),
            total_bytes: AtomicU64::new(total_bytes.unwrap_or(0)),
        }
    }

    pub(super) fn buffered_duration(&self, duration: Duration) -> Duration {
        let total = self.total_bytes.load(Ordering::Relaxed);
        if total == 0 {
            return Duration::ZERO;
        }
        let cached = self.cached_bytes.load(Ordering::Relaxed).min(total);
        duration.mul_f64(cached as f64 / total as f64)
    }
}

pub(super) fn near_estimated_end(position: u64, estimated_len: u64) -> bool {
    estimated_len > 0 && position.saturating_mul(100) >= estimated_len.saturating_mul(95)
}

pub(super) struct HttpRangeReader {
    control: Arc<StreamControl>,
    client: Client,
    url: String,
    response: Option<Response>,
    response_position: u64,
    position: u64,
    len: Option<u64>,
    range_supported: bool,
    decoder_seekable: bool,
    length_is_estimate: bool,
    mime_type: Option<String>,
    cache_file: Option<File>,
    cached_len: u64,
    complete_path: PathBuf,
    partial_path: PathBuf,
    cache_complete: bool,
    logged_cache_bucket: u64,
    pub(super) buffer_progress: Arc<BufferProgress>,
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

    pub(super) fn open_controlled(
        client: Client,
        url: String,
        cache_dir: &Path,
        cache_key: &str,
        control: Arc<StreamControl>,
    ) -> Result<Self> {
        control.check()?;
        let (complete_path, mut partial_path) = cache_paths(cache_dir, cache_key);
        if let Ok(file) = OpenOptions::new().read(true).open(&complete_path) {
            let len = file.metadata()?.len();
            log::info!(
                "audio cache hit; file={} bytes={len}",
                complete_path.display()
            );
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
                mime_type: None,
                cache_file: Some(file),
                cached_len: len,
                complete_path,
                partial_path,
                cache_complete: true,
                logged_cache_bucket: 10,
                buffer_progress: Arc::new(BufferProgress::new(len, Some(len))),
            });
        }

        let response = request_range(&client, &url, 0).context("failed to request audio stream")?;
        control.check()?;
        // Repeat-one and duplicate queue entries can preload the current song.
        // Never let a speculative reader modify its active reader's partial file.
        if control.speculative {
            static NEXT_PART: AtomicU64 = AtomicU64::new(0);
            let serial = NEXT_PART.fetch_add(1, Ordering::Relaxed);
            partial_path.set_extension(format!("preload-{}-{serial}.part", std::process::id()));
        }
        let range_supported = response.status() == StatusCode::PARTIAL_CONTENT;
        let len = content_range_len(&response).or_else(|| response.content_length());
        let length_is_estimate = is_transcoded_stream(&url);
        let decoder_seekable = range_supported && len.is_some() && !length_is_estimate;
        let mime_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let cache_file = fs::create_dir_all(cache_dir)
            .and_then(|()| {
                OpenOptions::new()
                    .create(true)
                    .read(true)
                    .write(true)
                    .truncate(false)
                    .open(&partial_path)
            })
            .map_err(|error| {
                log::warn!("audio cache is unavailable, continuing without it: {error}");
                error
            })
            .ok();
        let mut cached_len = cache_file
            .as_ref()
            .and_then(|file| file.metadata().ok())
            .map(|metadata| metadata.len())
            .unwrap_or(0);

        if !range_supported || len.is_some_and(|len| cached_len > len) {
            if cached_len > 0 {
                log::debug!(
                    "discarding partial audio cache; cached_bytes={cached_len} range_supported={range_supported} total_bytes={len:?}"
                );
            }
            if let Some(file) = cache_file.as_ref() {
                if let Err(error) = file.set_len(0) {
                    log::warn!("failed to reset partial audio cache: {error}");
                }
            }
            cached_len = 0;
        }

        log::info!(
            "audio network stream opened; endpoint={} status={} bytes={len:?} range_supported={range_supported} decoder_seekable={decoder_seekable} mime={mime_type:?} cached_bytes={cached_len}",
            stream_endpoint(&url),
            response.status()
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
            cache_file,
            cached_len,
            complete_path,
            partial_path,
            cache_complete: false,
            logged_cache_bucket: len
                .filter(|len| *len > 0)
                .map(|len| cached_len.saturating_mul(10) / len)
                .unwrap_or(0),
            buffer_progress: Arc::new(BufferProgress::new(cached_len, len)),
        })
    }

    pub(super) fn len(&self) -> Option<u64> {
        self.len
    }

    pub(super) fn is_seekable(&self) -> bool {
        self.cache_complete || self.decoder_seekable
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
        let response = request_range(&self.client, &self.url, position).map_err(|error| {
            let io = io_error(error);
            self.control
                .record_io_error(FailureError::new(classify_io(&io), io.to_string()));
            io
        })?;
        if position != 0 && response.status() != StatusCode::PARTIAL_CONTENT {
            return Err(failure_io(
                StreamFailure::Truncated,
                "audio server ignored the requested byte range",
            ));
        }
        self.range_supported |= response.status() == StatusCode::PARTIAL_CONTENT;
        self.response = Some(response);
        self.response_position = position;
        Ok(())
    }

    fn read_cached(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let available = self.cached_len.saturating_sub(self.position);
        let read_len = usize::try_from(available.min(buffer.len() as u64)).unwrap_or(buffer.len());
        let file = self
            .cache_file
            .as_mut()
            .ok_or_else(|| io::Error::other("audio cache file is closed"))?;
        file.seek(SeekFrom::Start(self.position))?;
        let read = file.read(&mut buffer[..read_len])?;
        self.position = self.position.saturating_add(read as u64);
        Ok(read)
    }

    fn finish_estimated_stream(&mut self) {
        let estimated_len = self.len;
        self.len = Some(self.position);
        self.buffer_progress
            .total_bytes
            .store(self.position, Ordering::Relaxed);
        self.logged_cache_bucket = 10;
        log::info!(
            "transcoded stream reached actual end; actual_bytes={} estimated_bytes={estimated_len:?}",
            self.position
        );
        log::info!(
            "audio cache progress; cached_bytes={} total_bytes={} percent=100",
            self.cached_len,
            self.position
        );
    }

    fn near_estimated_end(&self) -> bool {
        self.length_is_estimate
            && self
                .len
                .is_some_and(|len| near_estimated_end(self.position, len))
    }

    fn cache_network_bytes(&mut self, start: u64, bytes: &[u8]) -> io::Result<()> {
        if start != self.cached_len || bytes.is_empty() {
            return Ok(());
        }
        let Some(file) = self.cache_file.as_mut() else {
            return Ok(());
        };
        if let Err(error) = file
            .seek(SeekFrom::Start(start))
            .and_then(|_| file.write_all(bytes))
        {
            log::warn!("failed to write audio cache, continuing without it: {error}");
            self.cache_file = None;
            self.cached_len = 0;
            self.buffer_progress
                .cached_bytes
                .store(0, Ordering::Relaxed);
            return Ok(());
        }
        self.cached_len = self.cached_len.saturating_add(bytes.len() as u64);
        self.buffer_progress
            .cached_bytes
            .store(self.cached_len, Ordering::Relaxed);
        if let Some(total) = self.len.filter(|total| *total > 0) {
            let bucket = (self.cached_len.saturating_mul(10) / total).min(10);
            if bucket > self.logged_cache_bucket {
                self.logged_cache_bucket = bucket;
                log::info!(
                    "audio cache progress; cached_bytes={} total_bytes={total} percent={}",
                    self.cached_len,
                    bucket * 10
                );
            }
        }
        Ok(())
    }
}

impl Read for HttpRangeReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.control.check()?;
        if buffer.is_empty() {
            return Ok(0);
        }
        if self.position < self.cached_len {
            let position = self.position;
            match self.read_cached(buffer) {
                Ok(read) => return Ok(read),
                Err(error) if !self.cache_complete => {
                    log::warn!(
                        "failed to read partial audio cache, continuing without it: {error}"
                    );
                    self.cache_file = None;
                    self.cached_len = 0;
                    self.buffer_progress
                        .cached_bytes
                        .store(0, Ordering::Relaxed);
                    self.position = position;
                }
                Err(error) => return Err(error),
            }
        }
        if self.cache_complete || self.len == Some(self.position) {
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
                    self.cache_network_bytes(start, &buffer[..read])?;
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

impl Drop for HttpRangeReader {
    fn drop(&mut self) {
        let fully_cached = self
            .len
            .is_some_and(|len| len > 0 && self.cached_len >= len);
        if self.cache_complete {
            return;
        }
        self.cache_file.take();
        let unverified_preload =
            self.control.speculative && !self.control.fully_decoded.load(Ordering::Acquire);
        if !fully_cached || unverified_preload {
            if self.control.speculative {
                let _ = fs::remove_file(&self.partial_path);
            }
            return;
        }
        if let Err(error) = fs::rename(&self.partial_path, &self.complete_path) {
            log::warn!("failed to finalize audio cache: {error}");
        } else {
            log::info!(
                "audio cache finalized; file={} bytes={}",
                self.complete_path.display(),
                self.cached_len
            );
        }
    }
}

pub(super) fn request_range(client: &Client, url: &str, position: u64) -> Result<Response> {
    let response = client
        .get(url)
        .header(RANGE, format!("bytes={position}-"))
        .send()
        .map_err(|error| {
            anyhow!(FailureError::new(
                classify_reqwest(&error),
                t!(
                    "requesting the audio stream failed: %{error}",
                    error = error
                )
            ))
        })?;
    log::debug!(
        "audio HTTP response; endpoint={} range_start={position} status={} content_length={:?} content_range={:?} content_type={:?}",
        stream_endpoint(url),
        response.status(),
        response.content_length(),
        response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok()),
        response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
    );
    let status = response.status();
    if !status.is_success() {
        return Err(anyhow!(FailureError::new(
            StreamFailure::Http(status.as_u16()),
            t!("audio request returned HTTP %{status}", status = status)
        )));
    }
    // stream 端点正常必须返回音频二进制；HTTP 200 却给出 JSON/text
    // 说明服务器用 Subsonic 错误信封回应（例如拒绝/无法转码）。这里
    // 直接读出错误内容并中止，避免把错误正文喂给解码器后报出误导性的
    // “格式未识别”。归类为服务端拒绝，不触发自动重连。
    if response_is_error_envelope(&response) {
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        let mut body = String::new();
        let _ = response
            .take(8 * 1024)
            .read_to_string(&mut body)
            .map_err(|error| format!("body read failed: {error}"));
        let detail = subsonic_error_detail(&body);
        log::warn!(
            "stream endpoint returned a non-audio response; endpoint={} status={status} content_type={} body={}",
            stream_endpoint(url),
            content_type,
            body.trim()
        );
        return Err(anyhow!(FailureError::new(
            StreamFailure::Http(status.as_u16()),
            t!(
                "server refused the stream request: %{detail}",
                detail = detail
            ),
        )));
    }
    Ok(response)
}

/// 判定响应是否为非音频的错误信封：stream 端点正常必须返回音频二进制，
/// JSON/text 正文只可能是协议错误（或完全跑错的网关页）。
pub(super) fn response_is_error_envelope(response: &Response) -> bool {
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

/// 解析 Subsonic 错误信封，取出服务端给出的真实原因。
///
/// 形如 `{"subsonic-response":{"status":"failed","error":{"code":0,"message":"…"}}}`；
/// 解析不出时回退到截断后的原始正文，保证日志与界面仍能看到内容。
pub(super) fn subsonic_error_detail(body: &str) -> String {
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
    let Some(error) = error else {
        return raw();
    };
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

pub(super) fn content_range_len(response: &Response) -> Option<u64> {
    response
        .headers()
        .get(CONTENT_RANGE)?
        .to_str()
        .ok()?
        .rsplit_once('/')?
        .1
        .parse()
        .ok()
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
