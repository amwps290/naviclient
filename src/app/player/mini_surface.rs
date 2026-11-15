//! Cached device-pixel coverage for the mini window's transparent outline.

use std::sync::Arc;

use gpui_kit::{Hsla, RenderImage, Rgba, Window};
use image::{Frame, RgbaImage};

pub(super) const RADIUS: f32 = 14.0;
const SAMPLES: u32 = 8;
// A slightly wider reconstruction filter softens the visible staircase at 96
// DPI. Its width is in device pixels, so high-DPI edges stay equally precise.
const FILTER_WIDTH: f32 = 1.5;

#[derive(Clone, Copy, PartialEq)]
struct Key {
    width: u32,
    height: u32,
    scale: f32,
    tint: (Hsla, Hsla),
    border: Hsla,
}

#[derive(Default)]
pub(super) struct MiniSurface {
    cached: Option<(Key, Arc<RenderImage>)>,
}

impl MiniSurface {
    pub(super) fn image(
        &mut self,
        width: u32,
        height: u32,
        scale: f32,
        tint: (Hsla, Hsla),
        border: Hsla,
        window: &mut Window,
    ) -> Arc<RenderImage> {
        let key = Key {
            width,
            height,
            scale,
            tint,
            border,
        };
        if let Some((previous, image)) = &self.cached {
            if *previous == key {
                return image.clone();
            }
        }
        let image = Arc::new(RenderImage::new(vec![Frame::new(rasterize(key))]));
        self.release(window);
        self.cached = Some((key, image.clone()));
        image
    }

    pub(super) fn release(&mut self, window: &mut Window) {
        if let Some((_, image)) = self.cached.take() {
            if let Err(error) = window.drop_image(image) {
                log::warn!("failed to release mini surface texture: {error}");
            }
        }
    }
}

fn distance(x: f32, y: f32, width: f32, height: f32, radius: f32, inset: f32) -> f32 {
    let radius = radius
        .min(width / 2.0 - inset)
        .min(height / 2.0 - inset)
        .max(0.0);
    let dx = (x - width / 2.0).abs() - (width / 2.0 - inset - radius);
    let dy = (y - height / 2.0).abs() - (height / 2.0 - inset - radius);
    dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) - radius
}

fn coverage(x: f32, y: f32, width: f32, height: f32, radius: f32, inset: f32) -> f32 {
    let d = distance(x, y, width, height, radius, inset);
    if d <= -FILTER_WIDTH {
        return 1.0;
    }
    if d >= FILTER_WIDTH {
        return 0.0;
    }
    let mut inside = 0;
    for sy in 0..SAMPLES {
        for sx in 0..SAMPLES {
            let offset = |sample| ((sample as f32 + 0.5) / SAMPLES as f32 - 0.5) * FILTER_WIDTH;
            // Extend the edge texels when filtering, keeping the straight
            // perimeter opaque rather than creating a faint rectangular rim.
            let sample_x = (x + offset(sx)).clamp(0.0, width);
            let sample_y = (y + offset(sy)).clamp(0.0, height);
            inside += u32::from(distance(sample_x, sample_y, width, height, radius, inset) <= 0.0);
        }
    }
    inside as f32 / (SAMPLES * SAMPLES) as f32
}

fn rasterize(key: Key) -> RgbaImage {
    let mut pixels = RgbaImage::new(key.width, key.height);
    let width = key.width as f32;
    let height = key.height as f32;
    let radius = RADIUS * key.scale;
    let start = Rgba::from(key.tint.0);
    let end = Rgba::from(key.tint.1);
    let border = Rgba::from(key.border);
    // Match GPUI's sRGB 115-degree gradient, including its aspect correction.
    let angle = (115.0_f32 - 90.0).to_radians();
    let mut direction = (angle.cos(), angle.sin());
    if width > height {
        direction.1 *= height / width;
    } else {
        direction.0 *= width / height;
    }
    let length = direction.0.hypot(direction.1);
    let extent = if direction.0.abs() > direction.1.abs() {
        width
    } else {
        height
    };
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        let x = x as f32 + 0.5;
        let y = y as f32 + 0.5;
        let outer = coverage(x, y, width, height, radius, 0.0);
        let inner = coverage(
            x,
            y,
            width,
            height,
            (radius - key.scale).max(0.0),
            key.scale,
        );
        let t = (((x - width / 2.0) * direction.0 + (y - height / 2.0) * direction.1)
            / length
            / extent
            + 0.5)
            .clamp(0.0, 1.0);
        let mix = |a, b| a + (b - a) * t;
        let background = Rgba {
            r: mix(start.r, end.r),
            g: mix(start.g, end.g),
            b: mix(start.b, end.b),
            a: mix(start.a, end.a),
        };
        let rim = if outer > 0.0 {
            ((outer - inner) / outer).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let color = background.blend(Rgba {
            a: border.a * rim,
            ..border
        });
        let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
        // GPUI's atlas expects straight-alpha BGRA. Gradient and border share
        // one coverage mask; paint_image must not round this texture again.
        *pixel = image::Rgba([
            byte(color.b),
            byte(color.g),
            byte(color.r),
            byte(color.a * outer),
        ]);
    }
    pixels
}

#[cfg(test)]
mod tests;
