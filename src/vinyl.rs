//! Bounded rotation cache for GPUI versions without color-sprite transforms.
//! Frames arrive progressively, off the UI thread. Steady-state drawing only
//! blends two cached textures; there is no image encoding or per-tick rotation.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

use gpui_kit::{
    canvas, div, px, AnyElement, Corners, Image, IntoElement, ParentElement, RenderImage, Styled,
    Window,
};
use tokio::runtime::Runtime;

mod frames;
mod rotation;

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
}

impl Default for VinylCover {
    fn default() -> Self {
        Self {
            source_id: None,
            frames: vec![None; FRAMES],
            pending: None,
            failed: false,
            rotation: Rotation::default(),
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
            for frame in self.frames.iter_mut().filter_map(Option::take) {
                // Dropping the Arc alone does not release GPUI's atlas entries.
                if let Err(error) = window.drop_image(frame) {
                    log::warn!("failed to release vinyl textures: {error}");
                }
            }
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

#[cfg(test)]
mod tests;
