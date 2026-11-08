//! 封面加载：按需请求封面图、提取调色板、批量预取。

use super::artwork::decode_cover;
use crate::app::prelude::*;

/// 384 px 足够覆盖当前最大 180 logical-px 的网格封面（含 2x DPI 余量），
/// 比旧 500 px 解码纹理少约 41% 像素。
const COVER_REQUEST_SIZE: u32 = 384;
const MAX_COVER_PREFETCH_BATCH: usize = 80;

impl NavidromeApp {
    pub(in crate::app) fn ensure_cover(&mut self, cover_id: Option<&str>, want_palette: bool) {
        let source = self.api.clone();
        self.ensure_source_cover(source, cover_id, want_palette);
    }

    pub(in crate::app) fn ensure_song_cover(&mut self, song: &Song, want_palette: bool) {
        let source = self.source_for_song(song).cloned();
        self.ensure_source_cover(source, song.cover_art.as_deref(), want_palette);
    }

    fn ensure_source_cover(
        &mut self,
        api: Option<MusicSource>,
        cover_id: Option<&str>,
        want_palette: bool,
    ) {
        let Some(id) = cover_id else { return };
        let Some(api) = api else { return };
        if let Some(image) = self.state.covers.image(api.id(), id) {
            self.mark_cover_image_used(image);
        }
        if !self.state.covers.request_needed(api.id(), id, want_palette) {
            return;
        }
        let url = match api.cover_url(id, COVER_REQUEST_SIZE) {
            Ok(url) => url,
            Err(error) => {
                self.notify_error(t!(
                    "Cover request failed: %{error}",
                    error = format!("{error:#}")
                ));
                return;
            }
        };
        let Some(request) =
            self.state
                .covers
                .begin(api.id(), id, want_palette, &mut self.library_requests)
        else {
            return;
        };
        let id = id.to_string();
        let config = self.config.clone();
        let source = api.id().clone();
        let tx = self.tx.clone();
        let cover_slots = self.cover_slots.clone();
        self.spawn_future(async move {
            // 整条加载管线限流，而不只是解码：大收藏库若同时下载几百张封面，
            // 每个任务都会在等待解码许可时持有完整字节，造成明显瞬时内存峰值。
            let _permit = cover_slots.acquire_owned().await;
            // 先查磁盘缓存，命中跳过网络；未命中走网络并回写缓存。
            let result = match config::read_source_cover_cache(&config, &source, &id) {
                Some(bytes) => Ok(bytes),
                None => match api.get_bytes(&url).await {
                    Ok(bytes) => {
                        config::write_source_cover_cache(&config, &source, &id, &bytes);
                        Ok(bytes)
                    }
                    Err(error) => Err(error_message(error)),
                },
            };
            let result = match result {
                Ok(bytes) => Ok(decode_cover(bytes, want_palette)),
                Err(error) => Err(error),
            };
            let _ = tx.send(Msg::Cover { request, result });
        });
    }

    pub(in crate::app) fn finish_cover(
        &mut self,
        request: CoverRequest,
        result: Result<crate::msg::DecodedCover, String>,
    ) {
        let Some(update) = self.state.covers.complete(&request, result) else {
            return;
        };
        // Free the replaced image's retention slot before touching its successor.
        if let Some(previous) = update.replaced {
            self.queue_cover_asset_release(previous);
        }
        if let Some(image) = self.state.covers.image(request.source(), request.id()) {
            self.mark_cover_image_used(image);
        }
        if update.follow_up_palette {
            let api = self.source_by_id(request.source()).cloned();
            self.ensure_source_cover(api, Some(request.id()), true);
        }
    }

    pub(in crate::app) fn prune_source_covers(&mut self) {
        let sources = self
            .api
            .iter()
            .chain(self.playback_source.iter())
            .map(|api| api.id().clone())
            .collect();
        for image in self.state.covers.retain_sources(&sources) {
            self.queue_cover_asset_release(image);
        }
    }

    pub(in crate::app) fn preload_covers(&mut self, ids: Vec<String>) {
        // 网格封面只用于显示，不需要提取调色板（调色板仅播放页/详情页配色用）。
        // 单次数据响应最多预取 80 张；歌曲列表滚动时还会按可见范围继续补齐。
        for id in ids.into_iter().take(MAX_COVER_PREFETCH_BATCH) {
            self.ensure_cover(Some(&id), false);
        }
    }
}
