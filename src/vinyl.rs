//! Bounded rotation cache for GPUI versions without color-sprite transforms.
//! Frames arrive progressively, off the UI thread. Steady-state drawing only
//! blends two cached textures; there is no image encoding or per-tick rotation.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use gpui_kit::{
    canvas, div, px, AnyElement, Corners, Image, IntoElement, ParentElement, RenderImage, Styled,
    Window,
};
use image::{imageops::FilterType, DynamicImage, Frame, ImageDecoder, ImageReader, RgbaImage};
use tokio::runtime::Runtime;

// 25.3 MiB of pixel data, plus at most the same amount of atlas image data.
// The label is at most 184 logical pixels across; cap high-DPI rasterization
// too, rather than letting a retina display multiply the cache size by four.
const SIDE: u32 = 192;
const FRAMES: usize = 180;

struct Pending {
    cancel: Arc<AtomicBool>,
    result: mpsc::Receiver<anyhow::Result<(usize, Arc<RenderImage>)>>,
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub struct VinylCover {
    source_id: Option<u64>,
    frames: Vec<Option<Arc<RenderImage>>>,
    pending: Option<Pending>,
    failed: bool,
    phase: f32,
    last_tick: Option<Instant>,
}

impl Default for VinylCover {
    fn default() -> Self {
        Self {
            source_id: None,
            frames: vec![None; FRAMES],
            pending: None,
            failed: false,
            phase: 0.0,
            last_tick: None,
        }
    }
}

impl VinylCover {
    pub fn cancel_pending(&mut self) {
        self.pending = None;
    }

    /// Called by the existing UI poller, so the cache needs no animation timer.
    pub fn poll_ready(&mut self) -> bool {
        let Some(pending) = &self.pending else {
            return false;
        };
        let mut changed = false;
        loop {
            match pending.result.try_recv() {
                Ok(Ok((index, frame))) => {
                    self.frames[index] = Some(frame);
                    changed = true;
                }
                Ok(Err(error)) => {
                    log::warn!("failed to prepare vinyl cover: {error}");
                    self.failed = true;
                    self.pending = None;
                    break;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        changed
    }

    pub fn phase(&self) -> f32 {
        self.phase
    }

    pub fn tick(&mut self, now: Instant, spinning: bool) -> bool {
        if !spinning {
            self.last_tick = None;
            return false;
        }
        let Some(previous) = self.last_tick.replace(now) else {
            return false;
        };
        // Advance only through drawable frames. Startup, a slow worker, seeking,
        // and returning from a hidden window must never jump to a future angle.
        let elapsed = now.duration_since(previous).min(Duration::from_millis(100));
        let phase = (self.phase + elapsed.as_secs_f32() / 18.0).fract();
        let (first, next, _) = frame_pair(phase);
        if self.frames[first].is_some() && self.frames[next].is_some() {
            self.phase = phase;
            true
        } else {
            false
        }
    }

    pub fn prepare(
        &mut self,
        source: Arc<Image>,
        playing_visible: bool,
        runtime: &Runtime,
        window: &mut Window,
    ) {
        if self.source_id != Some(source.id) {
            self.cancel_pending();
            for frame in self.frames.iter_mut().filter_map(Option::take) {
                // Dropping the Arc alone does not release GPUI's atlas entries.
                if let Err(error) = window.drop_image(frame) {
                    log::warn!("failed to release vinyl textures: {error}");
                }
            }
            self.source_id = Some(source.id);
            self.failed = false;
            self.phase = 0.0;
            self.last_tick = None;
        }
        if !playing_visible {
            self.cancel_pending();
            return;
        }
        if self.pending.is_some() || self.failed || self.frames.iter().all(Option::is_some) {
            return;
        }
        let start = frame_pair(self.phase).0;
        let missing: Vec<_> = (0..FRAMES)
            .map(|offset| (start + offset) % FRAMES)
            .filter(|index| self.frames[*index].is_none())
            .collect();
        if missing.is_empty() {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (tx, result) = mpsc::sync_channel(4);
        self.pending = Some(Pending { cancel, result });
        runtime.spawn_blocking(move || {
            if let Err(error) =
                build_frames(&source.bytes, &worker_cancel, missing, |index, frame| {
                    tx.send(Ok((index, frame))).is_ok()
                })
            {
                let _ = tx.send(Err(error));
            }
        });
    }

    pub fn render(&self, side: f32, phase: f32) -> Option<AnyElement> {
        let (first, next, blend) = frame_pair(phase);
        let first = self.frames[first].as_ref()?.clone();
        let next = self.frames[next].as_ref().unwrap_or(&first).clone();
        let layer = |frame, opacity| {
            canvas(
                |_, _, _| (),
                move |bounds, (), window, _| {
                    if let Err(error) = window.paint_image(
                        bounds,
                        bounds,
                        Corners::all(bounds.size.width / 2.0),
                        frame,
                        0,
                        false,
                    ) {
                        log::warn!("failed to paint vinyl cover: {error}");
                    }
                },
            )
            .absolute()
            .inset_0()
            .opacity(opacity)
        };
        Some(
            div()
                .relative()
                .size(px(side))
                .flex_none()
                .child(layer(first, 1.0))
                .child(layer(next, blend))
                .into_any_element(),
        )
    }
}

fn frame_pair(phase: f32) -> (usize, usize, f32) {
    let frame = phase.rem_euclid(1.0) * FRAMES as f32;
    let first = frame.floor() as usize % FRAMES;
    (first, (first + 1) % FRAMES, frame.fract())
}

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

fn build_frames(
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
mod tests {
    use super::*;
    use image::{ImageFormat, Rgba};

    #[test]
    fn rotation_is_clockwise_and_keeps_center() {
        let mut source = RgbaImage::from_pixel(5, 5, Rgba([0, 0, 0, 255]));
        source[(2, 0)] = Rgba([10, 20, 30, 255]);
        source[(2, 2)] = Rgba([40, 50, 60, 255]);
        let rotated = rotate(&source, 0.25);
        assert_eq!(rotated[(4, 2)], source[(2, 0)]);
        assert_eq!(rotated[(2, 2)], source[(2, 2)]);
        assert_eq!(rotate(&source, 0.0)[(2, 0)], source[(2, 0)]);
    }

    #[test]
    fn frame_interpolation_wraps_without_a_jump() {
        assert_eq!(frame_pair(0.0), (0, 1, 0.0));
        assert_eq!(frame_pair(1.0), (0, 1, 0.0));
        let (first, next, blend) = frame_pair(179.5 / 180.0);
        assert_eq!((first, next), (179, 0));
        assert!((blend - 0.5).abs() < 0.0001);
    }

    #[test]
    fn portrait_and_landscape_are_center_cropped_and_bgra() {
        for (width, height) in [(12, 4), (4, 12)] {
            let mut source = RgbaImage::from_pixel(width, height, Rgba([255, 0, 0, 255]));
            for y in (height - 4) / 2..(height + 4) / 2 {
                for x in (width - 4) / 2..(width + 4) / 2 {
                    source[(x, y)] = Rgba([10, 20, 100, 255]);
                }
            }
            let mut bytes = Cursor::new(Vec::new());
            source.write_to(&mut bytes, ImageFormat::Png).unwrap();
            let pixels = cover_pixels(bytes.get_ref()).unwrap();
            assert_eq!(pixels.dimensions(), (SIDE, SIDE));
            assert_eq!(pixels[(SIDE / 2, SIDE / 2)].0, [100, 20, 10, 255]);
            assert_eq!(pixels[(SIDE / 4, SIDE / 4)].0, [100, 20, 10, 255]);
        }
    }

    #[test]
    fn cancelled_work_skips_even_decoding() {
        build_frames(&[], &AtomicBool::new(true), vec![0], |_, _| {
            panic!("cancelled work must not publish a frame")
        })
        .unwrap();
    }

    #[test]
    fn cancelling_pending_work_unblocks_a_full_channel() {
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, result) = mpsc::sync_channel(1);
        let pending = Pending {
            cancel: cancel.clone(),
            result,
        };
        let frame = Arc::new(RenderImage::new(vec![Frame::new(RgbaImage::new(2, 2))]));
        tx.send(Ok((0, frame.clone()))).unwrap();
        let producer = std::thread::spawn(move || tx.send(Ok((1, frame))));
        drop(pending);
        assert!(cancel.load(Ordering::Relaxed));
        assert!(producer.join().unwrap().is_err());
    }

    #[test]
    fn starts_with_two_frames_and_never_catches_up_after_waiting() {
        let mut cover = VinylCover::default();
        let start = Instant::now();
        assert!(!cover.tick(start, true));
        assert!(!cover.tick(start + Duration::from_secs(5), true));
        assert_eq!(cover.phase(), 0.0);

        let frame = Arc::new(RenderImage::new(vec![Frame::new(RgbaImage::new(2, 2))]));
        cover.frames[0] = Some(frame.clone());
        cover.frames[1] = Some(frame.clone());
        let ready = start + Duration::from_millis(5_050);
        assert!(cover.tick(ready, true));
        assert!((cover.phase() - 0.05 / 18.0).abs() < 0.00001);
        assert!(cover.frames[2].is_none()); // No full-cache requirement.

        let phase = cover.phase();
        assert!(!cover.tick(ready, false));
        assert!(!cover.tick(ready + Duration::from_secs(10), true));
        assert_eq!(cover.phase(), phase);
        assert!(cover.tick(ready + Duration::from_millis(10_040), true));

        let phase = cover.phase();
        assert!(!cover.tick(ready + Duration::from_millis(10_090), true));
        assert_eq!(cover.phase(), phase); // Wait for frame 2, without a jump.
        cover.frames[2] = Some(frame);
        assert!(cover.tick(ready + Duration::from_millis(10_140), true));
        assert!((cover.phase() - phase - 0.05 / 18.0).abs() < 0.00001);
    }

    #[test]
    fn publishes_frames_progressively_and_stops_when_receiver_closes() {
        let source = RgbaImage::from_pixel(4, 4, Rgba([10, 20, 30, 255]));
        let mut bytes = Cursor::new(Vec::new());
        source.write_to(&mut bytes, ImageFormat::Png).unwrap();
        let mut received = Vec::new();
        build_frames(
            bytes.get_ref(),
            &AtomicBool::new(false),
            vec![0, 1, 2],
            |index, frame| {
                assert_eq!(frame.frame_count(), 1);
                received.push(index);
                received.len() < 2
            },
        )
        .unwrap();
        assert_eq!(received, [0, 1]);
    }
}
