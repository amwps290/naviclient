//! Mix opaque rotation frames before applying the display's circular coverage.

use std::sync::Arc;

use anyhow::Context as _;
use gpui_kit::{ImageId, RenderImage, Window};
use image::{imageops, imageops::FilterType, Frame, RgbaImage};

#[derive(PartialEq, Eq)]
struct CompositeKey {
    first: ImageId,
    next: ImageId,
    blend: u32,
    side: u32,
}

#[derive(Default)]
pub(super) struct CompositeCache {
    key: Option<CompositeKey>,
    sources: [Option<RasterFrame>; 2],
    pub(super) displayed: Option<Arc<RenderImage>>,
}

#[derive(Clone)]
struct RasterFrame {
    id: ImageId,
    side: u32,
    pixels: Arc<RgbaImage>,
}

impl CompositeCache {
    pub(super) fn image(
        &mut self,
        first: &Arc<RenderImage>,
        next: &Arc<RenderImage>,
        blend: f32,
        raster_side: u32,
        window: &mut Window,
    ) -> anyhow::Result<Arc<RenderImage>> {
        let blend = if Arc::ptr_eq(first, next) {
            0.0
        } else {
            blend.clamp(0.0, 1.0)
        };
        // Never upscale the bounded rotation cache on high-DPI displays.
        let side = raster_side.max(1).min(first.size(0).width.0.max(0) as u32);
        let key = CompositeKey {
            first: first.id,
            next: next.id,
            blend: blend.to_bits(),
            side,
        };
        if self.key.as_ref() == Some(&key) {
            if let Some(image) = &self.displayed {
                return Ok(image.clone());
            }
        }
        let first_pixels = self.raster(first, side)?;
        let next_pixels = if Arc::ptr_eq(first, next) {
            first_pixels.clone()
        } else {
            self.raster(next, side)?
        };
        let pixels = mix(&first_pixels.pixels, &next_pixels.pixels, blend);
        let image = Arc::new(RenderImage::new(vec![Frame::new(pixels)]));
        self.release_displayed(window);
        // The old next frame becomes the new first frame during rotation.
        // Retain both reduced sources so filtering runs only on cache misses.
        self.sources = [Some(first_pixels), Some(next_pixels)];
        self.key = Some(key);
        self.displayed = Some(image.clone());
        Ok(image)
    }

    pub(super) fn release(&mut self, window: &mut Window) {
        self.release_displayed(window);
        self.sources = Default::default();
    }

    fn raster(&self, frame: &RenderImage, side: u32) -> anyhow::Result<RasterFrame> {
        if let Some(source) = self
            .sources
            .iter()
            .flatten()
            .find(|source| source.id == frame.id && source.side == side)
        {
            return Ok(source.clone());
        }
        Ok(RasterFrame {
            id: frame.id,
            side,
            pixels: Arc::new(rasterize(frame, side)?),
        })
    }

    fn release_displayed(&mut self, window: &mut Window) {
        self.key = None;
        if let Some(image) = self.displayed.take() {
            // GPUI retains atlas entries after the last Arc is dropped.
            if let Err(error) = window.drop_image(image) {
                log::warn!("failed to release composited vinyl texture: {error}");
            }
        }
    }
}

fn rasterize(frame: &RenderImage, side: u32) -> anyhow::Result<RgbaImage> {
    let size = frame.size(0);
    anyhow::ensure!(
        size.width.0 > 0 && size.height == size.width && side > 0,
        "invalid vinyl frame size"
    );
    let bytes = frame.as_bytes(0).context("missing vinyl frame")?;
    let pixels = RgbaImage::from_raw(size.width.0 as u32, size.height.0 as u32, bytes.to_vec())
        .context("invalid vinyl frame pixels")?;
    Ok(if size.width.0 as u32 == side {
        pixels
    } else {
        // A single bilinear GPU sample cannot adequately reduce 192px artwork
        // to the mini player's 60px label. Prefilter at its device-pixel size.
        imageops::resize(&pixels, side, side, FilterType::Lanczos3)
    })
}

fn mix(first: &RgbaImage, next: &RgbaImage, blend: f32) -> RgbaImage {
    let mut pixels = RgbaImage::new(first.width(), first.height());
    for (pixel, (first, next)) in pixels.pixels_mut().zip(first.pixels().zip(next.pixels())) {
        for channel in 0..3 {
            pixel[channel] = (first[channel] as f32 * (1.0 - blend) + next[channel] as f32 * blend)
                .round() as u8;
        }
        // Source artwork is flattened onto black before rotation. Leave the
        // entire square opaque so texture filtering never samples a hard mask.
        pixel[3] = 255;
    }
    pixels
}

#[cfg(test)]
mod tests;
