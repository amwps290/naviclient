//! Playback reporting and authenticated stream and artwork URLs.

use anyhow::Result;

use super::OpenSubsonicProvider;

impl OpenSubsonicProvider {
    /// 通知 Navidrome 当前正在播放的歌曲（更新“正在播放”列表）。
    pub async fn update_now_playing(&self, song_id: &str, time_secs: u32) -> Result<()> {
        self.get_json(
            "updateNowPlaying",
            &[("id", song_id), ("time", &time_secs.to_string())],
        )
        .await?;
        Ok(())
    }

    /// 向 Navidrome 提交 Scrobble（记录播放次数/最近播放）。submission=true 表示一次完整播放。
    pub async fn scrobble(&self, song_id: &str, submission: bool) -> Result<()> {
        let mut params = vec![("id", song_id)];
        if submission {
            params.push(("submission", "true"));
        }
        self.get_json("scrobble", &params).await?;
        Ok(())
    }

    pub fn stream_url(&self, id: &str, max_bit_rate: Option<u32>) -> Result<String> {
        let max_bit_rate = max_bit_rate.map(|value| value.to_string());
        let mut params = vec![("id", id), ("estimateContentLength", "true")];
        if let Some(max_bit_rate) = max_bit_rate.as_deref() {
            params.push(("maxBitRate", max_bit_rate));
            params.push(("format", "mp3"));
        }
        self.url_for("stream", &params)
    }

    pub fn cover_url(&self, id: &str, size: u32) -> Result<String> {
        self.url_for("getCoverArt", &[("id", id), ("size", &size.to_string())])
    }
}

#[cfg(test)]
mod tests;
