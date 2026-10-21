//! 封面加载：按需请求封面图、提取调色板、批量预取。

use super::*;

impl NavidromeApp {
    pub(super) fn ensure_cover(&mut self, cover_id: Option<&str>, want_palette: bool) {
        let Some(id) = cover_id else { return };
        // 已有可用数据就不请求：图像已在、或这次不需要调色板且调色板
        // 也在（见 cover_request_needed）。这让网格预取过的封面在之后
        // 播放时仍能补提调色板。
        if !cover_request_needed(
            self.state.covers.contains_key(id),
            self.state.cover_palettes.contains_key(id),
            want_palette,
        ) {
            return;
        }
        // requested_covers 表示“在途请求”，请求完成后由 Msg::Cover
        // 处理移除；同一时刻对同一封面只保留一个请求。
        if !self.state.requested_covers.insert(id.to_string()) {
            return;
        }
        let Some(api) = self.api.clone() else { return };
        let url = match api.cover_url(id, 500) {
            Ok(url) => url,
            Err(error) => {
                self.notify_error(t!(
                    "Cover request failed: %{error}",
                    error = format!("{error:#}")
                ));
                return;
            }
        };
        let id = id.to_string();
        let config = self.config.clone();
        let server_id = self.config.active_server_id().map(str::to_string);
        let tx = self.tx.clone();
        let cover_slots = self.cover_slots.clone();
        self.spawn_future(async move {
            // 网络下载不限并发（IO 等待不占 CPU），只对解码限流，避免占满后台线程。
            // 先查磁盘缓存，命中跳过网络；未命中走网络并回写缓存。
            let result = match config::read_cover_cache(&config, server_id.as_deref(), &id) {
                Some(bytes) => Ok(bytes),
                None => match api.get_bytes(&url).await {
                    Ok(bytes) => {
                        config::write_cover_cache(&config, server_id.as_deref(), &id, &bytes);
                        Ok(bytes)
                    }
                    Err(error) => Err(error_message(error)),
                },
            };
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
