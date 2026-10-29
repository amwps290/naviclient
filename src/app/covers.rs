//! 封面加载：按需请求封面图、提取调色板、批量预取。

use super::*;

/// 384 px 足够覆盖当前最大 180 logical-px 的网格封面（含 2x DPI 余量），
/// 比旧 500 px 解码纹理少约 41% 像素。
const COVER_REQUEST_SIZE: u32 = 384;
/// 96 张 384² BGRA 纹理约 54 MiB；旧磁盘缓存里的 500² 图片最坏约 92 MiB。
const MAX_DECODED_COVERS: usize = 96;
const MAX_COVER_PREFETCH_BATCH: usize = 80;
const MAX_FAILED_COVERS: usize = 256;

impl NavidromeApp {
    pub(super) fn ensure_cover(&mut self, cover_id: Option<&str>, want_palette: bool) {
        let Some(id) = cover_id else { return };
        if let Some(image) = self.state.covers.get(id) {
            self.mark_cover_image_used(image);
        }
        if self.state.failed_covers.contains(id) {
            return;
        }
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
        let Some(api) = self.api.clone() else { return };
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
        if !self.state.requested_covers.insert(id.to_string()) {
            return;
        }
        let id = id.to_string();
        let config = self.config.clone();
        let server_id = self.config.active_server_id().map(str::to_string);
        let tx = self.tx.clone();
        let cover_slots = self.cover_slots.clone();
        self.spawn_future(async move {
            // 整条加载管线限流，而不只是解码：大收藏库若同时下载几百张封面，
            // 每个任务都会在等待解码许可时持有完整字节，造成明显瞬时内存峰值。
            let _permit = cover_slots.acquire_owned().await;
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
                Ok(bytes) => Ok(decode_cover(bytes, want_palette)),
                Err(error) => Err(error),
            };
            let _ = tx.send(Msg::Cover { id, result });
        });
    }

    pub(super) fn preload_covers(&mut self, ids: Vec<String>) {
        // 网格封面只用于显示，不需要提取调色板（调色板仅播放页/详情页配色用）。
        // 单次数据响应最多预取 80 张；歌曲列表滚动时还会按可见范围继续补齐。
        for id in ids.into_iter().take(MAX_COVER_PREFETCH_BATCH) {
            self.ensure_cover(Some(&id), false);
        }
    }

    /// 标记图片刚刚参与渲染；超出上限的最久未使用纹理由 UI 轮询器释放。
    pub(super) fn mark_cover_image_used(&self, image: &Arc<GpuiImage>) {
        if image.id() == self.default_cover.id() {
            return;
        }
        if let Some(evicted) = touch_lru(
            &mut self.cover_image_lru.borrow_mut(),
            image.id(),
            MAX_DECODED_COVERS,
        ) {
            self.pending_cover_image_evictions
                .borrow_mut()
                .push_back(evicted);
        }
    }

    /// 从 GPUI 的资产缓存和窗口 atlas 同时移除过期封面。仅删除解码纹理，
    /// `state.covers` 中的压缩源字节仍保留；用户滚回旧位置时可以直接重解码，
    /// 不需要重新请求网络。
    pub(super) fn release_unused_cover_textures(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut released = HashSet::new();
        let releases = std::mem::take(&mut *self.pending_cover_asset_releases.borrow_mut());
        for image in releases {
            if released.insert(image.id()) {
                release_cover_texture(image, window, cx);
            }
        }
        let evictions = std::mem::take(&mut *self.pending_cover_image_evictions.borrow_mut());
        for image_id in evictions {
            // 排队后若又被当前帧使用，它已经重新进入 LRU，不应释放。
            if self.cover_image_lru.borrow().contains(&image_id) {
                continue;
            }
            let image = self
                .state
                .covers
                .values()
                .find(|image| image.id() == image_id)
                .cloned();
            let Some(image) = image else { continue };
            if released.insert(image.id()) {
                release_cover_texture(image, window, cx);
            }
        }
    }

    pub(super) fn queue_cover_asset_release(&self, image: Arc<GpuiImage>) {
        self.pending_cover_asset_releases
            .borrow_mut()
            .push_back(image);
    }

    pub(super) fn remember_failed_cover(&mut self, id: String) {
        if self.state.failed_covers.insert(id.clone()) {
            self.state.failed_cover_order.push_back(id);
        }
        while self.state.failed_covers.len() > MAX_FAILED_COVERS {
            let Some(oldest) = self.state.failed_cover_order.pop_front() else {
                break;
            };
            self.state.failed_covers.remove(&oldest);
        }
    }
}

fn release_cover_texture(
    image: Arc<GpuiImage>,
    window: &mut Window,
    cx: &mut Context<NavidromeApp>,
) {
    if let Some(render_image) = image.clone().get_render_image(window, cx) {
        if let Err(error) = window.drop_image(render_image) {
            log::warn!("failed to release cover texture: {error}");
        }
    }
    image.remove_asset(cx);
}
