//! Continuous rotation timing, independent of GPUI textures.

use std::time::{Duration, Instant};

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
        can_draw: impl FnOnce() -> bool,
    ) -> bool {
        if !spinning {
            self.last_tick = None;
            return false;
        }
        let Some(previous) = self.last_tick.replace(now) else {
            return false;
        };
        // Advance only when artwork is ready. Startup, a slow worker, seeking,
        // and returning from a hidden window must never jump to a future angle.
        let elapsed = now.duration_since(previous).min(Duration::from_millis(100));
        let phase = (self.phase + elapsed.as_secs_f32() / 18.0).fract();
        if can_draw() {
            self.phase = phase;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests;
