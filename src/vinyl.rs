//! Bounded single-source rotation for GPUI without color-sprite transforms.
//! Artwork is decoded off the UI thread, then sampled at the exact visible
//! angle at display size. Only current and outgoing textures enter the GPU.

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
use frames::build_frame;
use rotation::Rotation;

struct Pending {
    cancel: Arc<AtomicBool>,
    result: mpsc::Receiver<anyhow::Result<Arc<RenderImage>>>,
}

impl Drop for Pending {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub struct VinylCover {
    source_id: Option<u64>,
    source: Option<Arc<RenderImage>>,
    pending: Option<Pending>,
    failed: bool,
    rotation: Rotation,
    composite: Rc<RefCell<CompositeCache>>,
    frozen: Option<FrozenCover>,
}

/// Retain the exact visible angle and texture while the old record is removed.
struct FrozenCover {
    source: Arc<RenderImage>,
    phase: f32,
    composite: Rc<RefCell<CompositeCache>>,
}

impl FrozenCover {
    fn render(&self, side: f32) -> AnyElement {
        render_source(
            side,
            self.source.clone(),
            self.phase,
            self.composite.clone(),
        )
    }
}

impl Default for VinylCover {
    fn default() -> Self {
        Self {
            source_id: None,
            source: None,
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
        if let Some(source) = self.source.as_ref() {
            self.frozen = Some(FrozenCover {
                source: source.clone(),
                phase: self.phase(),
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

    /// Called by the existing UI poller, so decoding needs no separate timer.
    pub fn poll_ready(&mut self) -> bool {
        let Some(pending) = &self.pending else {
            return false;
        };
        match pending.result.try_recv() {
            Ok(Ok(source)) => {
                self.source = Some(source);
                self.pending = None;
                true
            }
            Ok(Err(error)) => {
                log::warn!("failed to prepare vinyl cover: {error}");
                self.failed = true;
                self.pending = None;
                true
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.pending = None;
                false
            }
            Err(mpsc::TryRecvError::Empty) => false,
        }
    }

    pub fn phase(&self) -> f32 {
        self.rotation.phase()
    }

    pub fn tick(&mut self, now: Instant, spinning: bool) -> bool {
        self.rotation.tick(now, spinning, || self.source.is_some())
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
            // A frozen cover retains the old source and its own texture until
            // the record exchange finishes.
            self.source = None;
            self.source_id = Some(source.id);
            self.failed = false;
            self.rotation.reset();
        }
        if !playing_visible {
            self.cancel_pending();
            return;
        }
        if self.pending.is_some() || self.failed || self.source.is_some() {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (tx, result) = mpsc::sync_channel(1);
        self.pending = Some(Pending { cancel, result });
        runtime.spawn_blocking(move || match build_frame(&source.bytes, &worker_cancel) {
            Ok(Some(frame)) => {
                let _ = tx.send(Ok(frame));
            }
            Ok(None) => {}
            Err(error) => {
                let _ = tx.send(Err(error));
            }
        });
    }

    pub fn render(&self, side: f32, phase: f32) -> Option<AnyElement> {
        Some(render_source(
            side,
            self.source.as_ref()?.clone(),
            phase,
            self.composite.clone(),
        ))
    }
}

fn render_source(
    side: f32,
    source: Arc<RenderImage>,
    phase: f32,
    composite: Rc<RefCell<CompositeCache>>,
) -> AnyElement {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let raster_side = (f32::from(bounds.size.width) * window.scale_factor()).ceil() as u32;
            let result = composite
                .borrow_mut()
                .image(&source, phase, raster_side, window)
                .and_then(|image| {
                    // Clip once, after filtering and rotating the opaque source.
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
