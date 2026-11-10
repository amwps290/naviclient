//! Structured playback failures and error classification across I/O layers.

use std::io;
use std::sync::{Arc, Mutex};

use rust_i18n::t;

pub(super) type SharedStreamError = Arc<Mutex<Option<FailureError>>>;

/// 播放失败的结构化原因，用来在界面上区分「本地网络」「服务端」「解码」三类问题。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamFailure {
    /// 无法建立连接：DNS 失败、拒绝连接、无路由。
    Connect,
    /// 连接已建立但迟迟没有响应，或读取中途超时。
    Timeout,
    /// 服务端返回了错误状态码。
    Http(u16),
    /// 连接还在，但长时间没有音频数据到达。
    NoData,
    /// 数据提前结束，通常是代理/转码截断了响应。
    Truncated,
    /// 下载到的内容无法解码。
    Decode,
    /// 本地音频输出设备问题。
    Output,
    /// 本地播放准备无法启动，例如系统线程资源不足。
    Preparation,
    /// A signed address expired before a new HTTP range request.
    ResourceExpired,
}

impl StreamFailure {
    /// 界面上的短标题（翻译键）。
    pub fn summary(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Connect => t!("Cannot reach the server"),
            Self::Timeout => t!("Server is not responding"),
            Self::Http(_) => t!("Server returned an error"),
            Self::NoData => t!("Server stopped sending audio"),
            Self::Truncated => t!("Audio stream was cut off"),
            Self::Decode => t!("Cannot decode this track"),
            Self::Output => t!("Audio output failed"),
            Self::Preparation => t!("Audio preparation failed"),
            Self::ResourceExpired => t!("Audio playback address expired"),
        }
    }

    /// 归因提示：帮助用户判断是网络还是服务端的问题（翻译键）。
    pub fn cause(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Connect => t!("Networking: the server could not be reached. Check Wi-Fi/VPN and the server URL."
            ),
            Self::Timeout => t!("Networking or server: the connection opened but the reply timed out."
            ),
            Self::Http(status) if status >= 500 => t!("Server: the server reported an internal error. Retry later."
            ),
            Self::Http(_) => t!("Server: the request was rejected by the server."),
            Self::NoData => t!("Server: the connection is alive but no audio is arriving (transcoding or overloaded)."
            ),
            Self::Truncated => t!("Networking or server: the stream ended early, usually a proxy or idle timeout."
            ),
            Self::Decode => t!("Client: the received data is not decodable audio."),
            Self::Output => t!("Local: the audio device failed; unrelated to the network."),
            Self::Preparation => t!("Local: audio could not be prepared. Retry playback or restart the app."),
            Self::ResourceExpired => t!("A fresh playback address is required. Retry playback."),
        }
    }

    /// 是否可以自动重连。网络抖动、超时、服务端 5xx、流被截断都值得重试；
    /// 解码失败与本地输出问题重试无意义。
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            Self::Connect | Self::Timeout | Self::NoData | Self::Truncated
        ) || matches!(self, Self::Http(status) if status >= 500 || status == 408 || status == 429)
    }
}

/// 携带分类的播放错误；`Display` 只输出人类可读的细节，分类通过 downcast 取回。
#[derive(Clone, Debug)]
pub(crate) struct FailureError {
    pub(crate) kind: StreamFailure,
    pub(crate) detail: String,
}

impl FailureError {
    pub(super) fn new(kind: StreamFailure, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }

    /// 完整的单行描述，形如「Server stopped sending audio: no data for 30s」。
    pub(crate) fn message(&self) -> String {
        format!("{}: {}", self.kind.summary(), self.detail)
    }
}

impl std::fmt::Display for FailureError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for FailureError {}

/// 把带分类的错误包成 `io::Error`，让 reader 的调用链不丢失原因。
pub(super) fn failure_io(kind: StreamFailure, detail: impl Into<String>) -> io::Error {
    io::Error::other(FailureError::new(kind, detail))
}

pub(super) fn classify_reqwest(error: &reqwest::Error) -> StreamFailure {
    if let Some(status) = error.status() {
        return StreamFailure::Http(status.as_u16());
    }
    if error.is_timeout() {
        return StreamFailure::Timeout;
    }
    if error.is_connect() {
        return StreamFailure::Connect;
    }
    if error.is_body() || error.is_decode() {
        return StreamFailure::Truncated;
    }
    StreamFailure::Connect
}

pub(super) fn classify_io(error: &io::Error) -> StreamFailure {
    if let Some(failure) = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<FailureError>())
    {
        return failure.kind;
    }
    if let Some(reqwest) = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<reqwest::Error>())
    {
        return classify_reqwest(reqwest);
    }
    match error.kind() {
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => StreamFailure::Timeout,
        io::ErrorKind::ConnectionRefused
        | io::ErrorKind::ConnectionReset
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::NotConnected
        | io::ErrorKind::AddrNotAvailable => StreamFailure::Connect,
        io::ErrorKind::UnexpectedEof => StreamFailure::Truncated,
        _ => StreamFailure::Decode,
    }
}

/// 从 anyhow 错误里尽可能还原分类。
pub(super) fn classify_anyhow(error: &anyhow::Error) -> StreamFailure {
    if let Some(failure) = error.downcast_ref::<FailureError>() {
        return failure.kind;
    }
    if let Some(io) = error.downcast_ref::<io::Error>() {
        return classify_io(io);
    }
    if let Some(reqwest) = error.downcast_ref::<reqwest::Error>() {
        return classify_reqwest(reqwest);
    }
    let text = format!("{error:#}").to_ascii_lowercase();
    if text.contains("timed out") || text.contains("timeout") {
        StreamFailure::Timeout
    } else if text.contains("connect") || text.contains("dns") || text.contains("resolve") {
        StreamFailure::Connect
    } else {
        StreamFailure::Decode
    }
}
