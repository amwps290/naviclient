//! Cache a single-angle texture before applying the display's circular coverage.

use std::sync::Arc;

use anyhow::Context as _;
use gpui_kit::{ImageId, RenderImage, Window};
use image::{imageops, imageops::FilterType, Frame, RgbaImage};

use super::frames::rotate;

#[derive(PartialEq, Eq)]
struct CompositeKey {
    source: ImageId,
    phase: u32,
    side: u32,
}

#[derive(Default)]
pub(super) struct CompositeCache {
    key: Option<CompositeKey>,
    source: Option<RasterFrame>,
    pub(super) displayed: Option<Arc<RenderImage>>,
}

#[derive(Clone)]
struct RasterFrame {
    id: ImageId,
    side: u32,
    pixels: Arc<RgbaImage>,
}

impl CompositeCache {
    pub(super) fn memory_bytes(&self) -> usize {
        let source_bytes = self
            .source
            .as_ref()
            .map_or(0, |frame| frame.pixels.as_raw().len());
        let displayed_bytes = self
            .displayed
            .as_ref()
            .and_then(|img| img.as_bytes(0).map(<[u8]>::len))
            .unwrap_or(0);
        source_bytes.saturating_add(displayed_bytes)
    }

    pub(super) fn image(
        &mut self,
        source: &Arc<RenderImage>,
        phase: f32,
        raster_side: u32,
        window: &mut Window,
    ) -> anyhow::Result<Arc<RenderImage>> {
        let phase = phase.rem_euclid(1.0);
        // Never upscale the bounded rotation cache on high-DPI displays.
        let side = raster_side.max(1).min(source.size(0).width.0.max(0) as u32);
        let key = CompositeKey {
            source: source.id,
            phase: phase.to_bits(),
            side,
        };
        if self.key.as_ref() == Some(&key) {
            if let Some(image) = &self.displayed {
                return Ok(image.clone());
            }
        }
        let source = self.raster(source, side)?;
        // Interpolating colors between displaced frames produces two copies of
        // text and edges. Sample one source at the requested geometry instead.
        let pixels = rotate(&source.pixels, phase);
        let image = Arc::new(RenderImage::new(vec![Frame::new(pixels)]));
        self.release_displayed(window);
        self.source = Some(source);
        self.key = Some(key);
        self.displayed = Some(image.clone());
        Ok(image)
    }

    pub(super) fn release(&mut self, window: &mut Window) {
        self.release_displayed(window);
        self.source = None;
    }

    fn raster(&self, frame: &RenderImage, side: u32) -> anyhow::Result<RasterFrame> {
        if let Some(source) = self
            .source
            .as_ref()
            .filter(|source| source.id == frame.id && source.side == side)
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

#[cfg(test)]
mod tests;
