//! 切歌系统通知（Linux D-Bus org.freedesktop.Notifications）。
//!
//! 设计要点：
//! - **后台发送**：单个线程顺序发送，只保留最新待发送通知；
//!   D-Bus 调用和封面文件检查不阻塞界面轮询。
//! - **同 ID 替换防堆积**：快速连续切歌时，后续通知携带上一条的原始
//!   ID，通知服务器按规范用新内容替换旧通知，不产生堆积。
//! - **失败静默**：通知是纯增益（无 D-Bus 会话、服务器忙碌、被
//!   do-not-disturb 拒绝等），任何失败只记日志，绝不影响播放。
//! - **封面走磁盘缓存路径**：封面下载管线已有 `{cache_dir}/covers/`
//!   磁盘缓存（md5 文件名），通知直接引用文件路径，通过 image-path
//!   hint 展示大图；缓存未命中时回退默认图标。
//!
//! Windows 通知（winrt 管线）暂缓，等真实 Windows 使用场景。

/// 通知正文：艺术家与专辑组合；任一为空时只显示存在的那个。
/// 纯逻辑，便于测试。
pub(crate) fn notification_body(artist: &str, album: &str) -> String {
    let artist = artist.trim();
    let album = album.trim();
    match (artist.is_empty(), album.is_empty()) {
        (false, false) => t!("%{artist} — %{album}", artist = artist, album = album).to_string(),
        (false, true) => artist.to_string(),
        (true, false) => album.to_string(),
        (true, true) => t!("Now playing").to_string(),
    }
}

#[cfg(any(target_os = "linux", test))]
mod worker;

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub(crate) use linux::TrackNotifier;

#[cfg(not(target_os = "linux"))]
pub(crate) struct TrackNotifier;

// `notification_body` 在所有平台都要取词（测试也会走它），因此这个
// import 不能跟着 Linux 实现一起 cfg 掉。
use rust_i18n::t;
#[cfg(not(target_os = "linux"))]
use std::path::Path;

#[cfg(not(target_os = "linux"))]
impl TrackNotifier {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn track_changed(&mut self, _title: &str, _body: &str, _cover_path: Option<&Path>) {}

    pub(crate) fn cancel_pending(&self) {}
}

#[cfg(test)]
mod tests {
    use super::notification_body;

    #[test]
    fn notification_body_combines_artist_and_album() {
        assert_eq!(notification_body("Artist", "Album"), "Artist — Album");
        assert_eq!(notification_body("Artist", "  "), "Artist");
        assert_eq!(notification_body("", "Album"), "Album");
        assert_eq!(notification_body("", ""), "Now playing");
    }
}
