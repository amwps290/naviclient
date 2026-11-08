//! Resolved media transport. These values are intentionally not serializable:
//! signed addresses and request headers must never enter saved queues/config.

use std::fmt;
use std::ops::Deref;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use md5::{Digest, Md5};
use reqwest::header::HeaderMap;

use super::MediaKey;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CachePolicy {
    #[default]
    Persistent,
    /// A private scratch file, released with the reader. Only this many bytes
    /// of a contiguous prefix may be retained; further reads use HTTP Range.
    Temporary { max_bytes: u64 },
}

#[derive(Clone)]
pub struct HttpResource {
    inner: Arc<HttpResourceData>,
}

#[derive(Clone)]
struct HttpResourceData {
    url: String,
    headers: HeaderMap,
    cache_policy: CachePolicy,
    mime_hint: Option<String>,
    length_is_estimate: bool,
    proxy: Option<String>,
    backups: Vec<HttpResource>,
}

impl HttpResource {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            inner: Arc::new(HttpResourceData {
                url: url.into(),
                headers: HeaderMap::new(),
                cache_policy: CachePolicy::Persistent,
                mime_hint: None,
                length_is_estimate: false,
                proxy: None,
                backups: Vec::new(),
            }),
        }
    }

    pub fn with_headers(mut self, headers: HeaderMap) -> Self {
        Arc::make_mut(&mut self.inner).headers = headers;
        self
    }

    pub fn with_proxy(mut self, proxy: Option<String>) -> Self {
        Arc::make_mut(&mut self.inner).proxy = proxy;
        self
    }

    pub fn with_backups(mut self, backups: Vec<HttpResource>) -> Self {
        Arc::make_mut(&mut self.inner).backups = backups;
        self
    }

    pub fn backups(&self) -> &[HttpResource] {
        &self.inner.backups
    }

    pub fn with_cache_policy(mut self, policy: CachePolicy) -> Self {
        Arc::make_mut(&mut self.inner).cache_policy = policy;
        self
    }

    pub fn with_mime_hint(mut self, hint: Option<String>) -> Self {
        Arc::make_mut(&mut self.inner).mime_hint = hint;
        self
    }

    pub fn with_estimated_length(mut self, estimated: bool) -> Self {
        Arc::make_mut(&mut self.inner).length_is_estimate = estimated;
        self
    }

    pub fn cache_policy(&self) -> CachePolicy {
        self.inner.cache_policy
    }

    pub fn mime_hint(&self) -> Option<&str> {
        self.inner.mime_hint.as_deref()
    }

    pub fn length_is_estimate(&self) -> bool {
        self.inner.length_is_estimate
    }

    pub fn url(&self) -> &str {
        &self.inner.url
    }

    pub fn headers(&self) -> &HeaderMap {
        &self.inner.headers
    }

    pub fn proxy(&self) -> Option<&str> {
        self.inner.proxy.as_deref()
    }
}

impl From<String> for HttpResource {
    fn from(url: String) -> Self {
        Self::new(url)
    }
}

impl From<&str> for HttpResource {
    fn from(url: &str) -> Self {
        Self::new(url)
    }
}

impl Deref for HttpResource {
    type Target = str;

    fn deref(&self) -> &str {
        self.url()
    }
}

impl fmt::Debug for HttpResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpResource")
            .field("url", &"[redacted]")
            .field("headers", &"[redacted]")
            .field("proxy", &self.proxy().map(|_| "[redacted]"))
            .field("cache_policy", &self.cache_policy())
            .field("mime_hint", &self.mime_hint())
            .field("length_is_estimate", &self.length_is_estimate())
            .field("backup_count", &self.backups().len())
            .finish()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AudioQuality {
    pub codec: Option<String>,
    /// Declared bandwidth in bits/s, not a measured bitrate or fixed tier.
    pub bandwidth: Option<u64>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
}

#[derive(Clone, Debug)]
pub struct PlaybackResource {
    pub media: MediaKey,
    /// Provider representation (including transcoding profile when applicable).
    pub representation: String,
    pub http: HttpResource,
    pub backups: Vec<HttpResource>,
    pub quality: AudioQuality,
    pub duration: Option<Duration>,
    pub expires_at: Option<SystemTime>,
}

impl PlaybackResource {
    pub fn cache_key(&self) -> String {
        let mut hash = Md5::new();
        hash.update(self.media.cache_key().as_bytes());
        hash.update((self.representation.len() as u64).to_le_bytes());
        hash.update(self.representation.as_bytes());
        format!("{:x}", hash.finalize())
    }
}

#[cfg(test)]
mod tests;
