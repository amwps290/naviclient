//! Record exchange and tonearm motion, driven by the existing UI poller.

use std::time::{Duration, Instant};

const LIFT: Duration = Duration::from_millis(300);
const REMOVE: Duration = Duration::from_millis(600);
const INSERT: Duration = Duration::from_millis(700);
const LOWER: Duration = Duration::from_millis(400);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RecordPose {
    /// Offsets are fractions of the record's diameter.
    pub x: f32,
    pub y: f32,
    pub opacity: f32,
}

impl Default for RecordPose {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            opacity: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MotionFrame {
    pub record: RecordPose,
    pub outgoing: bool,
    pub arm_degrees: f32,
}

struct Exchange {
    started: Instant,
    record_from: RecordPose,
    arm_from: f32,
}

pub(crate) struct TurntableMotion {
    now: Instant,
    arm_degrees: f32,
    target_degrees: f32,
    exchange: Option<Exchange>,
}

impl Default for TurntableMotion {
    fn default() -> Self {
        Self {
            now: Instant::now(),
            arm_degrees: 0.0,
            target_degrees: 0.0,
            exchange: None,
        }
    }
}

impl TurntableMotion {
    /// Returns whether the newly outgoing record needs a fresh cover snapshot.
    /// Rapid skips during removal replace only the destination record.
    pub(crate) fn begin_exchange(&mut self) -> bool {
        let frame = self.frame();
        if frame.outgoing {
            return false;
        }
        self.exchange = Some(Exchange {
            started: self.now,
            record_from: frame.record,
            arm_from: frame.arm_degrees,
        });
        true
    }

    pub(crate) fn exchanging(&self) -> bool {
        self.exchange.is_some()
    }

    pub(crate) fn tick(&mut self, now: Instant, visible: bool, target_degrees: f32) -> bool {
        let before = self.frame();
        let elapsed = now
            .saturating_duration_since(self.now)
            .min(Duration::from_millis(100));
        self.now = now;
        self.target_degrees = target_degrees;
        if !visible {
            // Returning to the page shows the current record, without replaying
            // exchanges that happened while hidden, in mini mode or in settings.
            self.exchange = None;
            self.arm_degrees = target_degrees;
        } else if self.exchange.as_ref().is_some_and(|exchange| {
            now.saturating_duration_since(exchange.started) >= LIFT + REMOVE + INSERT + LOWER
        }) {
            self.exchange = None;
            self.arm_degrees = target_degrees;
        } else {
            // Smooth seeks, pause and resume from the angle currently on screen.
            let weight = 1.0 - (-elapsed.as_secs_f32() / 0.10).exp();
            self.arm_degrees += (target_degrees - self.arm_degrees) * weight;
            if (target_degrees - self.arm_degrees).abs() < 0.005 {
                self.arm_degrees = target_degrees;
            }
        }
        before != self.frame()
    }

    pub(crate) fn frame(&self) -> MotionFrame {
        let Some(exchange) = &self.exchange else {
            return MotionFrame {
                record: RecordPose::default(),
                outgoing: false,
                arm_degrees: self.arm_degrees,
            };
        };
        let elapsed = self.now.saturating_duration_since(exchange.started);
        if elapsed < LIFT {
            MotionFrame {
                record: exchange.record_from,
                outgoing: true,
                arm_degrees: exchange.arm_from * (1.0 - ease(fraction(elapsed, LIFT))),
            }
        } else if elapsed < LIFT + REMOVE {
            let progress = ease(fraction(elapsed - LIFT, REMOVE));
            MotionFrame {
                record: interpolate(
                    exchange.record_from,
                    RecordPose {
                        x: -1.15,
                        y: -0.15,
                        opacity: 0.0,
                    },
                    progress,
                ),
                outgoing: true,
                arm_degrees: 0.0,
            }
        } else if elapsed < LIFT + REMOVE + INSERT {
            let progress = ease(fraction(elapsed - LIFT - REMOVE, INSERT));
            MotionFrame {
                record: interpolate(
                    RecordPose {
                        x: 1.15,
                        y: -0.15,
                        opacity: 0.0,
                    },
                    RecordPose::default(),
                    progress,
                ),
                outgoing: false,
                arm_degrees: 0.0,
            }
        } else {
            MotionFrame {
                record: RecordPose::default(),
                outgoing: false,
                arm_degrees: self.target_degrees
                    * ease(fraction(elapsed - LIFT - REMOVE - INSERT, LOWER)),
            }
        }
    }
}

/// With the current SVG and pivot, 14° places the stylus on the outer groove
/// (radius ~0.44 diameters); 37° reaches the inner groove (~0.28), outside the label.
pub(crate) fn tonearm_target(playing: bool, position: Duration, duration: Option<Duration>) -> f32 {
    if !playing {
        return 0.0;
    }
    let progress = duration
        .filter(|duration| !duration.is_zero())
        .map_or(0.0, |duration| {
            (position.as_secs_f64() / duration.as_secs_f64()).clamp(0.0, 1.0) as f32
        });
    14.0 + 23.0 * progress
}

fn fraction(elapsed: Duration, duration: Duration) -> f32 {
    (elapsed.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
}

fn ease(value: f32) -> f32 {
    value * value * (3.0 - 2.0 * value)
}

fn interpolate(from: RecordPose, to: RecordPose, progress: f32) -> RecordPose {
    RecordPose {
        x: from.x + (to.x - from.x) * progress,
        y: from.y + (to.y - from.y) * progress,
        opacity: from.opacity + (to.opacity - from.opacity) * progress,
    }
}

#[cfg(test)]
mod tests;
