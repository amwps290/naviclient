//! Bounded, cancellable raster preparation for the current vinyl cover.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::RenderImage;
use image::{imageops::FilterType, DynamicImage, Frame, ImageDecoder, ImageReader, RgbaImage};

use super::rotation::FRAMES;

// The label is at most 184 logical pixels across; cap high-DPI rasterization
// rather than letting a retina display multiply the cache size by four.
const SIDE: u32 = 192;

fn cover_pixels(bytes: &[u8]) -> anyhow::Result<RgbaImage> {
    let mut decoder = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()?
        .into_decoder()?;
    let orientation = decoder.orientation()?;
    let mut decoded = DynamicImage::from_decoder(decoder)?;
    decoded.apply_orientation(orientation);
    let mut pixels = decoded
        .resize_to_fill(SIDE, SIDE, FilterType::Triangle)
        .into_rgba8();
    for pixel in pixels.pixels_mut() {
        // Opaque BGRA, flattened onto the black record. This makes source-over
        // blending of neighboring frames correct even for transparent artwork.
        let [red, green, blue, alpha] = pixel.0;
        let flatten = |channel: u8| ((channel as u16 * alpha as u16 + 127) / 255) as u8;
        pixel.0 = [flatten(blue), flatten(green), flatten(red), 255];
    }
    Ok(pixels)
}

pub(super) fn build_frames(
    bytes: &[u8],
    cancel: &AtomicBool,
    missing: Vec<usize>,
    mut publish: impl FnMut(usize, Arc<RenderImage>) -> bool,
) -> anyhow::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Ok(());
    }
    let started = Instant::now();
    let source = cover_pixels(bytes)?;
    for index in missing {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let frame_start = Instant::now();
        let frame = Frame::new(rotate(&source, index as f32 / FRAMES as f32));
        if !publish(index, Arc::new(RenderImage::new(vec![frame]))) {
            return Ok(());
        }
        // Bound the one-off preparation burst to roughly 25% of one CPU core.
        // Cancellation is checked every small sleep slice, including on shutdown.
        let rest = frame_start.elapsed() * 3;
        let deadline = Instant::now() + rest;
        while Instant::now() < deadline {
            if cancel.load(Ordering::Relaxed) {
                return Ok(());
            }
            std::thread::sleep(
                deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(5)),
            );
        }
    }
    log::debug!("vinyl rotation cache prepared in {:?}", started.elapsed());
    Ok(())
}

fn rotate(source: &RgbaImage, phase: f32) -> RgbaImage {
    let side = source.width();
    let center = (side - 1) as f32 * 0.5;
    let radius_squared = (side as f32 * 0.5 + 1.0).powi(2);
    let (sin, cos) = (phase * std::f32::consts::TAU).sin_cos();
    let mut output = RgbaImage::new(side, side);
    for (x, y, pixel) in output.enumerate_pixels_mut() {
        let dx = x as f32 - center;
        let dy = y as f32 - center;
        if dx * dx + dy * dy > radius_squared {
            continue;
        }
        // Inverse mapping gives clockwise rotation, matching GPUI's SVG matrix.
        let sx = (cos * dx + sin * dy + center).clamp(0.0, (side - 1) as f32);
        let sy = (-sin * dx + cos * dy + center).clamp(0.0, (side - 1) as f32);
        let x0 = sx as u32;
        let y0 = sy as u32;
        let x1 = (x0 + 1).min(side - 1);
        let y1 = (y0 + 1).min(side - 1);
        let fx = sx.fract();
        let fy = sy.fract();
        for channel in 0..3 {
            let top = source[(x0, y0)][channel] as f32 * (1.0 - fx)
                + source[(x1, y0)][channel] as f32 * fx;
            let bottom = source[(x0, y1)][channel] as f32 * (1.0 - fx)
                + source[(x1, y1)][channel] as f32 * fx;
            pixel[channel] = (top * (1.0 - fy) + bottom * fy).round() as u8;
        }
        pixel[3] = 255;
    }
    output
}

#[cfg(test)]
mod tests;
