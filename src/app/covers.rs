//! 封面加载：按需请求封面图、提取调色板、批量预取。

use super::*;

impl NavidromeApp {
    pub(super) fn ensure_cover(&mut self, cover_id: Option<&str>, want_palette: bool) {
        let Some(id) = cover_id else { return };
        if !self.state.requested_covers.insert(id.to_string()) {
            return;
        }
        let Some(api) = self.api.clone() else { return };
        let url = match api.cover_url(id, 500) {
            Ok(url) => url,
            Err(error) => {
                self.notify_error(format!("Cover request failed: {error:#}"));
                return;
            }
        };
        let id = id.to_string();
        let tx = self.tx.clone();
        let cover_slots = self.cover_slots.clone();
        self.spawn_future(async move {
            // 网络下载不限并发（IO 等待不占 CPU），只对解码限流，避免占满后台线程。
            let result = api.get_bytes(&url).await.map_err(error_message);
            let result = match result {
                Ok(bytes) => {
                    let _permit = cover_slots.acquire_owned().await;
                    Ok(decode_cover(bytes, want_palette))
                }
                Err(error) => Err(error),
            };
            let _ = tx.send(Msg::Cover { id, result });
        });
    }

    pub(super) fn preload_covers(&mut self, ids: Vec<String>) {
        // 网格封面只用于显示，不需要提取调色板（调色板仅播放页/详情页配色用）。
        for id in ids {
            self.ensure_cover(Some(&id), false);
        }
    }
}
