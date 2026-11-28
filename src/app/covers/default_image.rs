//! Prefilter the default artwork before GPUI's single-level texture sampling.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use anyhow::Context as _;
use gpui_kit::RenderImage;
use image::{imageops, imageops::FilterType, Frame, RgbaImage};

// Baked levels avoid repeatedly filtering the 1024px master on the UI thread.
// The final resize uses the smallest level at least as large as the display.
const LEVELS: &[(u32, &[u8])] = &[
    (64, include_bytes!("../../../assets/default-cover-64.png")),
    (128, include_bytes!("../../../assets/default-cover-128.png")),
    (256, include_bytes!("../../../assets/default-cover-256.png")),
    (512, include_bytes!("../../../assets/default-cover-512.png")),
    (1024, include_bytes!("../../../assets/default-cover.png")),
];
const CAPACITY: usize = 16;

#[derive(Default)]
pub(crate) struct DefaultCoverThumbnails {
    levels: HashMap<u32, RgbaImage>,
    images: VecDeque<(u32, Arc<RenderImage>)>,
    active: HashSet<u32>,
}

pub(crate) fn device_side(size: f32, scale: f32) -> u32 {
    (size * scale).round().clamp(1.0, 1024.0) as u32
}

impl DefaultCoverThumbnails {
    pub(crate) fn memory_bytes(&self) -> usize {
        let levels_bytes: usize = self.levels.values().map(|img| img.as_raw().len()).sum();
        let images_bytes: usize = self
            .images
            .iter()
            .filter_map(|(_, img)| img.as_bytes(0).map(<[u8]>::len))
            .sum();
        levels_bytes.saturating_add(images_bytes)
    }

    pub(crate) fn image(&mut self, side: u32) -> anyhow::Result<Arc<RenderImage>> {
        let side = side.clamp(1, 1024);
        self.active.insert(side);
        if let Some(index) = self.images.iter().position(|(size, _)| *size == side) {
            let entry = self.images.remove(index).expect("existing thumbnail");
            let image = entry.1.clone();
            self.images.push_front(entry);
            return Ok(image);
        }

        let &(level, bytes) = LEVELS.iter().find(|(level, _)| *level >= side).unwrap();
        if let std::collections::hash_map::Entry::Vacant(entry) = self.levels.entry(level) {
            let mut pixels = image::load_from_memory(bytes)
                .context("invalid default cover mip level")?
                .to_rgba8();
            // RenderImage and GPUI's image atlas store straight-alpha BGRA.
            for pixel in pixels.pixels_mut() {
                pixel.0.swap(0, 2);
            }
            entry.insert(pixels);
        }
        let source = &self.levels[&level];
        let pixels = if side == level {
            source.clone()
        } else {
            imageops::resize(source, side, side, FilterType::Lanczos3)
        };
        let image = Arc::new(RenderImage::new(vec![Frame::new(pixels)]));
        self.images.push_front((side, image.clone()));
        Ok(image)
    }

    /// Call before rendering the next scene, never while painting its covers.
    /// Keep all sizes used in the preceding scene, even if it exceeds the budget;
    /// removing an atlas entry already referenced by a scene corrupts its images.
    pub(crate) fn take_releases(&mut self) -> Vec<Arc<RenderImage>> {
        let mut releases = Vec::new();
        for index in (0..self.images.len()).rev() {
            if self.images.len() <= CAPACITY {
                break;
            }
            if !self.active.contains(&self.images[index].0) {
                releases.push(self.images.remove(index).expect("existing thumbnail").1);
            }
        }
        self.active.clear();
        releases
    }
}

#[cfg(test)]
mod tests;
