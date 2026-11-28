//! Background filtering of static artwork before single-level GPU sampling.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Cursor;
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Context as _;
use gpui_kit::{
    img, App, Image, ImageFormat, ImageSource, IntoElement, ObjectFit, RenderImage, RenderOnce,
    Task, Window,
};
use image::{imageops, imageops::FilterType, DynamicImage, Frame, ImageBuffer, ImageDecoder, ImageReader, Rgba, RgbaImage};

use crate::app::prelude::*;

const CAPACITY: usize = 160;
const BYTE_BUDGET: usize = 24 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    source: u64,
    side: u32,
    wide: bool,
}

struct Entry {
    image: Option<Arc<RenderImage>>,
    // Dropping an evicted entry cancels publication of its pending result.
    _task: Task<()>,
}

#[derive(Default)]
pub(in crate::app) struct CoverThumbnails {
    entries: HashMap<Key, Entry>,
    recent: VecDeque<Key>,
    active: HashSet<Key>,
    releases: Vec<Arc<RenderImage>>,
    bytes: usize,
}

impl CoverThumbnails {
    pub(in crate::app) fn stats(&self) -> (usize, usize) {
        (self.entries.len(), self.bytes)
    }

    pub(in crate::app) fn clear_inactive(&mut self) {
        let stale: Vec<_> = self
            .recent
            .iter()
            .filter(|key| !self.active.contains(key))
            .copied()
            .collect();
        for key in stale {
            self.remove(key);
        }
    }

    fn image(
        cache: &Rc<RefCell<Self>>,
        source: &Arc<Image>,
        side: u32,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Arc<RenderImage>> {
        Self::image_shape(cache, source, side, false, window, cx)
    }

    fn image_shape(
        cache: &Rc<RefCell<Self>>,
        source: &Arc<Image>,
        side: u32,
        wide: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Arc<RenderImage>> {
        let key = Key {
            source: source.id(),
            side: side.max(1),
            wide,
        };
        {
            let mut cache = cache.borrow_mut();
            cache.active.insert(key);
            cache.recent.retain(|previous| *previous != key);
            if let Some(entry) = cache.entries.get(&key) {
                let image = entry.image.clone();
                cache.recent.push_front(key);
                return image;
            }
        }
        // Retain GPUI's native animated-image path for GIF artwork.
        if source.format == ImageFormat::Gif {
            return None;
        }
        // Decode & filter directly from the compressed cover bytes on a background thread
        // without populating GPUI's global AssetCache or uploading the full-size source to the GPU atlas.
        let source_image = source.clone();
        let worker = cx.background_executor().spawn(async move {
            let decoded = decode_bgra(&source_image)?;
            let pixels = if key.wide {
                filter_wide_rgba(&decoded, key.side)
            } else {
                filter_rgba(&decoded, key.side)
            };
            pixels.map(|pixels| Arc::new(RenderImage::new(vec![pixels])))
        });
        let weak_cache = Rc::downgrade(cache);
        let task = window.spawn(cx, async move |cx| {
            let result = worker.await;
            if let Some(cache) = weak_cache.upgrade() {
                let _ = cx.update(|window, _| {
                    let mut cache = cache.borrow_mut();
                    if let Some(entry) = cache.entries.get_mut(&key) {
                        match result {
                            Ok(image) => {
                                let bytes = image.as_bytes(0).map_or(0, <[u8]>::len);
                                entry.image = Some(image);
                                cache.bytes += bytes;
                            }
                            Err(error) => log::warn!("failed to filter cover thumbnail: {error:#}"),
                        }
                        window.refresh();
                    }
                });
            }
        });
        let mut cache = cache.borrow_mut();
        cache.recent.push_front(key);
        cache.entries.insert(
            key,
            Entry {
                image: None,
                _task: task,
            },
        );
        None
    }

    pub(in crate::app) fn retire(&mut self, source: u64) {
        let keys: Vec<_> = self
            .entries
            .keys()
            .filter(|key| key.source == source)
            .copied()
            .collect();
        for key in keys {
            self.remove(key);
        }
    }

    fn remove(&mut self, key: Key) {
        self.recent.retain(|previous| *previous != key);
        self.active.remove(&key);
        if let Some(entry) = self.entries.remove(&key) {
            if let Some(image) = entry.image {
                self.bytes -= image.as_bytes(0).map_or(0, <[u8]>::len);
                self.releases.push(image);
            }
        }
    }

    // Run before building the next scene. Never release a texture referenced by
    // the preceding scene, even if an unusually large visible set exceeds budget.
    pub(in crate::app) fn take_releases(&mut self) -> Vec<Arc<RenderImage>> {
        let stale: Vec<_> = self
            .recent
            .iter()
            .rev()
            .filter(|key| !self.active.contains(key))
            .copied()
            .collect();
        for key in stale {
            if self.entries.len() <= CAPACITY && self.bytes <= BYTE_BUDGET {
                break;
            }
            self.remove(key);
        }
        self.active.clear();
        std::mem::take(&mut self.releases)
    }
}

fn decode_bgra(source: &Image) -> anyhow::Result<RgbaImage> {
    let mut decoder = ImageReader::new(Cursor::new(&source.bytes))
        .with_guessed_format()
        .context("failed to detect cover image format")?
        .into_decoder()
        .context("failed to initialize cover image decoder")?;
    let orientation = decoder.orientation().ok();
    let mut decoded = DynamicImage::from_decoder(decoder).context("failed to decode cover")?;
    if let Some(orientation) = orientation {
        decoded.apply_orientation(orientation);
    }
    let mut pixels = decoded.into_rgba8();
    // GPUI's RenderImage & sprite atlases store BGRA pixel order.
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Ok(pixels)
}

#[cfg(test)]
fn filter(source: &RenderImage, side: u32) -> anyhow::Result<Frame> {
    let size = source.size(0);
    let width = u32::try_from(size.width.0)?;
    let height = u32::try_from(size.height.0)?;
    anyhow::ensure!(width > 0 && height > 0, "empty cover image");
    let bytes = source.as_bytes(0).context("missing cover pixels")?;
    let image = ImageBuffer::<Rgba<u8>, &[u8]>::from_raw(width, height, bytes)
        .context("invalid cover pixels")?;
    filter_buffer(&image, side)
}

fn filter_rgba(image: &RgbaImage, side: u32) -> anyhow::Result<Frame> {
    filter_buffer(image, side)
}

fn filter_buffer<C: std::ops::Deref<Target = [u8]>>(
    image: &ImageBuffer<Rgba<u8>, C>,
    side: u32,
) -> anyhow::Result<Frame> {
    let (width, height) = image.dimensions();
    anyhow::ensure!(width > 0 && height > 0, "empty cover image");
    let crop_side = width.min(height);
    let crop = imageops::crop_imm(
        image,
        (width - crop_side).div_ceil(2),
        (height - crop_side).div_ceil(2),
        crop_side,
        crop_side,
    );
    // Keep the center crop used by ObjectFit::Cover; don't invent detail by
    // upscaling the source. Original bytes remain available for larger slots.
    let side = side.max(1).min(crop_side);
    Ok(Frame::new(imageops::resize(
        &*crop,
        side,
        side,
        FilterType::Lanczos3,
    )))
}

#[derive(IntoElement)]
pub(in crate::app) struct FilteredCover {
    pub(in crate::app) source: Arc<Image>,
    pub(in crate::app) cache: Rc<RefCell<CoverThumbnails>>,
    pub(in crate::app) size: f32,
    pub(in crate::app) radius: f32,
    pub(in crate::app) shadows: Vec<gpui_kit::BoxShadow>,
}

fn placeholder_bg(cx: &App) -> Hsla {
    if cx.has_global::<gpui_kit::component::Theme>() {
        palette(cx).sunken
    } else {
        hsla(0.0, 0.0, 0.5, 0.12)
    }
}

impl RenderOnce for FilteredCover {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let side = (self.size * window.scale_factor()).round().max(1.0) as u32;
        if let Some(image) = CoverThumbnails::image(&self.cache, &self.source, side, window, cx) {
            return img(ImageSource::Render(image))
                .size(px(self.size))
                .aspect_square()
                .object_fit(ObjectFit::Cover)
                .flex_none()
                .rounded(px(self.radius))
                .shadow(self.shadows)
                .into_any_element();
        }
        if self.source.format == ImageFormat::Gif {
            return img(self.source)
                .size(px(self.size))
                .aspect_square()
                .object_fit(ObjectFit::Cover)
                .flex_none()
                .rounded(px(self.radius))
                .shadow(self.shadows)
                .into_any_element();
        }
        // While the filtered thumbnail is decoding in the background, paint a lightweight
        // placeholder instead of uploading the full-resolution source into GPUI's GPU atlas.
        div()
            .size(px(self.size))
            .flex_none()
            .rounded(px(self.radius))
            .bg(placeholder_bg(cx))
            .shadow(self.shadows)
            .into_any_element()
    }
}

#[cfg(test)]
fn filter_wide(source: &RenderImage, side: u32) -> anyhow::Result<Frame> {
    let size = source.size(0);
    let width = u32::try_from(size.width.0)?;
    let height = u32::try_from(size.height.0)?;
    anyhow::ensure!(width > 0 && height > 0, "empty video artwork");
    let image = ImageBuffer::<Rgba<u8>, &[u8]>::from_raw(
        width,
        height,
        source.as_bytes(0).context("missing video artwork pixels")?,
    )
    .context("invalid video artwork pixels")?;
    filter_wide_buffer(&image, side)
}

fn filter_wide_rgba(image: &RgbaImage, side: u32) -> anyhow::Result<Frame> {
    filter_wide_buffer(image, side)
}

fn filter_wide_buffer<C: std::ops::Deref<Target = [u8]>>(
    image: &ImageBuffer<Rgba<u8>, C>,
    side: u32,
) -> anyhow::Result<Frame> {
    let (width, height) = image.dimensions();
    anyhow::ensure!(width > 0 && height > 0, "empty video artwork");
    let crop_width = width.min(height.saturating_mul(16) / 9).max(1);
    let crop_height = height.min(crop_width.saturating_mul(9) / 16).max(1);
    let crop = imageops::crop_imm(
        image,
        (width - crop_width).div_ceil(2),
        (height - crop_height).div_ceil(2),
        crop_width,
        crop_height,
    );
    let output_width = side.max(1).min(crop_width);
    let output_height = (output_width.saturating_mul(9) / 16)
        .max(1)
        .min(crop_height);
    Ok(Frame::new(imageops::resize(
        &*crop,
        output_width,
        output_height,
        FilterType::Lanczos3,
    )))
}

#[derive(IntoElement)]
pub(in crate::app) struct FilteredVideoCover {
    pub source: Arc<Image>,
    pub cache: Rc<RefCell<CoverThumbnails>>,
    pub width: f32,
    pub radius: f32,
    pub shadows: Vec<gpui_kit::BoxShadow>,
}

impl RenderOnce for FilteredVideoCover {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let side = (self.width * window.scale_factor()).round().max(1.0) as u32;
        if let Some(image) =
            CoverThumbnails::image_shape(&self.cache, &self.source, side, true, window, cx)
        {
            return img(ImageSource::Render(image))
                .w(px(self.width))
                .h(px(self.width * 9.0 / 16.0))
                .object_fit(ObjectFit::Cover)
                .flex_none()
                .rounded(px(self.radius))
                .shadow(self.shadows)
                .into_any_element();
        }
        if self.source.format == ImageFormat::Gif {
            return img(self.source)
                .w(px(self.width))
                .h(px(self.width * 9.0 / 16.0))
                .object_fit(ObjectFit::Cover)
                .flex_none()
                .rounded(px(self.radius))
                .shadow(self.shadows)
                .into_any_element();
        }
        div()
            .w(px(self.width))
            .h(px(self.width * 9.0 / 16.0))
            .flex_none()
            .rounded(px(self.radius))
            .bg(placeholder_bg(cx))
            .shadow(self.shadows)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
