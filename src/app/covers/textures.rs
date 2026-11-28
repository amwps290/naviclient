//! Scheduling and applying paired releases from GPUI's window atlas and asset cache.

use crate::app::prelude::*;

/// 48 retained cover sources keeps decoded artwork bounded while avoiding re-decode churn.
pub(in crate::app) const MAX_DECODED_COVERS: usize = 48;

impl NavidromeApp {
    pub(in crate::app) fn mark_cover_image_used(&self, image: &Arc<GpuiImage>) {
        self.cover_textures.borrow_mut().mark_used(image);
    }

    pub(in crate::app) fn queue_cover_asset_release(&self, image: Arc<GpuiImage>) {
        self.cover_textures.borrow_mut().retire(image);
    }

    /// Only decoded textures are released; cached source bytes remain available
    /// to decode again when the user scrolls back to an evicted cover.
    pub(in crate::app) fn release_unused_cover_textures(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let releases = self.cover_textures.borrow_mut().take_releases();
        for image in releases {
            self.cover_thumbnails.borrow_mut().retire(image.id());
            if let Some(render_image) = image.clone().get_render_image(window, cx) {
                if let Err(error) = window.drop_image(render_image) {
                    log::warn!("failed to release cover texture: {error}");
                }
            }
            image.remove_asset(cx);
        }
    }

    /// Release cached cover bytes, decoded GPU textures, and inactive thumbnails
    /// while preserving the currently playing track's cover.
    pub(in crate::app) fn purge_cover_memory_cache(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (usize, u64) {
        let mut protected_keys = HashSet::new();
        let mut protected_ids = HashSet::new();
        if let Some(song) = &self.state.now_playing {
            if let (Some(cover_id), Some(source)) = (
                song.cover_art.as_deref(),
                song.source.as_ref().or_else(|| self.session.source()),
            ) {
                protected_keys.insert(crate::source::MediaKey::new(source.clone(), cover_id));
                if let Some(img) = self.state.covers.image(source, cover_id) {
                    protected_ids.insert(img.id());
                }
            }
        }
        let (_, before_cover_bytes) = self.state.covers.stats();
        let (_, before_thumb_bytes) = self.cover_thumbnails.borrow().stats();

        let removed = self.state.covers.clear_except(&protected_keys);
        let removed_count = removed.len();
        for image in removed {
            self.queue_cover_asset_release(image);
        }
        self.cover_textures
            .borrow_mut()
            .evict_all_except(&protected_ids);
        self.cover_thumbnails.borrow_mut().clear_inactive();
        self.release_unused_cover_textures(window, cx);
        for image in self.cover_thumbnails.borrow_mut().take_releases() {
            if let Err(error) = window.drop_image(image) {
                log::warn!("failed to release purged cover thumbnail: {error}");
            }
        }

        let (_, after_cover_bytes) = self.state.covers.stats();
        let (_, after_thumb_bytes) = self.cover_thumbnails.borrow().stats();
        let freed = before_cover_bytes
            .saturating_sub(after_cover_bytes)
            .saturating_add(before_thumb_bytes.saturating_sub(after_thumb_bytes))
            as u64;
        (removed_count, freed)
    }
}
