//! Rotation timing and adjacent-frame selection, independent of GPUI textures.

use std::time::{Duration, Instant};

pub(super) const FRAMES: usize = 120;

#[derive(Default)]
pub(super) struct Rotation {
    phase: f32,
    last_tick: Option<Instant>,
}

impl Rotation {
    pub(super) fn phase(&self) -> f32 {
        self.phase
    }

    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn tick(
        &mut self,
        now: Instant,
        spinning: bool,
        can_draw: impl FnOnce(usize, usize) -> bool,
    ) -> bool {
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
        if can_draw(first, next) {
            self.phase = phase;
            true
        } else {
            false
        }
    }
}

pub(super) fn frame_pair(phase: f32) -> (usize, usize, f32) {
    let frame = phase.rem_euclid(1.0) * FRAMES as f32;
    let first = frame.floor() as usize % FRAMES;
    (first, (first + 1) % FRAMES, frame.fract())
}

#[cfg(test)]
mod tests;
