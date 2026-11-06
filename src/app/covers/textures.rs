//! Scheduling and applying paired releases from GPUI's window atlas and asset cache.

use crate::app::prelude::*;

/// 96 decoded 384² BGRA images use about 54 MiB; old 500² cache files can reach 92 MiB.
pub(in crate::app) const MAX_DECODED_COVERS: usize = 96;

impl NavidromeApp {
    pub(in crate::app) fn mark_cover_image_used(&self, image: &Arc<GpuiImage>) {
        self.cover_textures.borrow_mut().mark_used(image);
    }

    pub(in crate::app) fn queue_cover_asset_release(&self, image: Arc<GpuiImage>) {
        self.cover_textures.borrow_mut().retire(image);
    }

    pub(in crate::app) fn retire_server_covers(&self) {
        self.cover_textures
            .borrow_mut()
            .reset(self.state.covers.images().cloned());
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
            if let Some(render_image) = image.clone().get_render_image(window, cx) {
                if let Err(error) = window.drop_image(render_image) {
                    log::warn!("failed to release cover texture: {error}");
                }
            }
            image.remove_asset(cx);
        }
    }
}
