//! Bounded rotation cache for GPUI versions without color-sprite transforms.
//! Frames arrive progressively, off the UI thread. Drawing composites their
//! pixels at display size and applies one circular mask, without image encoding
//! or per-tick rotation. Only the current and outgoing composites enter the GPU.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

use gpui_kit::{canvas, px, AnyElement, Corners, Image, IntoElement, RenderImage, Styled, Window};
use tokio::runtime::Runtime;

mod composite;
mod frames;
pub(crate) mod motion;
mod rotation;

use composite::CompositeCache;
use frames::build_frames;
use rotation::{frame_pair, Rotation, FRAMES};

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
    rotation: Rotation,
    composite: Rc<RefCell<CompositeCache>>,
    frozen: Option<FrozenCover>,
}

/// Retain the exact visible angle and texture while the old record is removed.
struct FrozenCover {
    first: Arc<RenderImage>,
    next: Arc<RenderImage>,
    blend: f32,
    composite: Rc<RefCell<CompositeCache>>,
}

impl FrozenCover {
    fn render(&self, side: f32) -> AnyElement {
        render_pair(
            side,
            self.first.clone(),
            self.next.clone(),
            self.blend,
            self.composite.clone(),
        )
    }
}

impl Default for VinylCover {
    fn default() -> Self {
        Self {
            source_id: None,
            frames: vec![None; FRAMES],
            pending: None,
            failed: false,
            rotation: Rotation::default(),
            composite: Default::default(),
            frozen: None,
        }
    }
}

impl VinylCover {
    pub(crate) fn freeze(&mut self, window: &mut Window) {
        self.release_frozen(window);
        let (first, next, blend) = frame_pair(self.phase());
        if let Some(first) = self.frames[first].as_ref() {
            self.frozen = Some(FrozenCover {
                first: first.clone(),
                next: self.frames[next].as_ref().unwrap_or(first).clone(),
                blend,
                // The outgoing and incoming covers must own separate textures:
                // both can be referenced by the same scene during an exchange.
                composite: std::mem::take(&mut self.composite),
            });
        }
    }

    pub(crate) fn render_frozen(&self, side: f32) -> Option<AnyElement> {
        self.frozen.as_ref().map(|frozen| frozen.render(side))
    }

    pub(crate) fn release_frozen(&mut self, window: &mut Window) {
        let Some(frozen) = self.frozen.take() else {
            return;
        };
        frozen.composite.borrow_mut().release(window);
    }

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
        self.rotation.phase()
    }

    pub fn tick(&mut self, now: Instant, spinning: bool) -> bool {
        self.rotation.tick(now, spinning, |first, next| {
            self.frames[first].is_some() && self.frames[next].is_some()
        })
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
            self.composite.borrow_mut().release(window);
            // Cached rotations stay on the CPU. A frozen cover retains the
            // two sources and its own composite until the exchange finishes.
            self.frames.fill(None);
            self.source_id = Some(source.id);
            self.failed = false;
            self.rotation.reset();
        }
        if !playing_visible {
            self.cancel_pending();
            return;
        }
        if self.pending.is_some() || self.failed || self.frames.iter().all(Option::is_some) {
            return;
        }
        let start = frame_pair(self.rotation.phase()).0;
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
        Some(render_pair(
            side,
            first,
            next,
            blend,
            self.composite.clone(),
        ))
    }
}

fn render_pair(
    side: f32,
    first: Arc<RenderImage>,
    next: Arc<RenderImage>,
    blend: f32,
    composite: Rc<RefCell<CompositeCache>>,
) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let raster_side = (f32::from(bounds.size.width) * window.scale_factor()).ceil() as u32;
            let result = composite
                .borrow_mut()
                .image(&first, &next, blend, raster_side, window)
                .and_then(|image| {
                    // Apply coverage once, after mixing. Independently masked
                    // layers made the rim thicken as the blend approached 1.
                    window.paint_image(
                        bounds,
                        bounds,
                        Corners::all(bounds.size.width / 2.0),
                        image,
                        0,
                        false,
                    )
                });
            if let Err(error) = result {
                log::warn!("failed to paint vinyl cover: {error}");
            }
        },
    )
    .size(px(side))
    .flex_none()
    .into_any_element()
}

#[cfg(test)]
mod tests;
