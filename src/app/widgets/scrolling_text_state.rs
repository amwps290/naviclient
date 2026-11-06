//! Bounded font-aware text measurements and scrolling-label timing.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use gpui_kit::{Font, Pixels, SharedString};

const TEXT_WIDTH_CACHE_LIMIT: usize = 20_000;
const TEXT_GAP: f32 = 32.0;
const SCROLL_SPEED: f32 = 28.0;
const INITIAL_HOLD: f32 = 1.5;
const MINIMUM_PERIOD: f32 = 4.0;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TextWidthKey {
    font: Font,
    size_bits: u32,
    text: SharedString,
}

struct Measurement {
    width: Pixels,
    last_used: u64,
}

pub(in crate::app) struct TextWidthCache {
    entries: HashMap<TextWidthKey, Measurement>,
    recency: BTreeMap<u64, TextWidthKey>,
    serial: u64,
    limit: usize,
}

impl Default for TextWidthCache {
    fn default() -> Self {
        Self::with_limit(TEXT_WIDTH_CACHE_LIMIT)
    }
}

impl TextWidthCache {
    fn with_limit(limit: usize) -> Self {
        Self {
            entries: HashMap::new(),
            recency: BTreeMap::new(),
            serial: 0,
            limit,
        }
    }

    pub(in crate::app) fn measure(
        &mut self,
        font: Font,
        size: Pixels,
        text: SharedString,
        measure: impl FnOnce() -> Pixels,
    ) -> Pixels {
        self.serial = self
            .serial
            .checked_add(1)
            .expect("text cache serial exhausted");
        let serial = self.serial;
        let key = TextWidthKey {
            font,
            size_bits: f32::from(size).to_bits(),
            text,
        };
        if let Some(entry) = self.entries.get_mut(&key) {
            self.recency.remove(&entry.last_used);
            entry.last_used = serial;
            self.recency.insert(serial, key);
            return entry.width;
        }

        let width = measure();
        if self.limit == 0 || !f32::from(width).is_finite() || f32::from(width) < 0.0 {
            return width;
        }
        if self.entries.len() == self.limit {
            let (_, oldest) = self
                .recency
                .pop_first()
                .expect("full cache has an oldest entry");
            self.entries.remove(&oldest);
        }
        self.recency.insert(serial, key.clone());
        self.entries.insert(
            key,
            Measurement {
                width,
                last_used: serial,
            },
        );
        width
    }
}

#[derive(Clone, Copy, Debug)]
pub(in crate::app) struct ScrollingTextMotion {
    pub(in crate::app) gap: f32,
    pub(in crate::app) travel: f32,
    pub(in crate::app) period: Duration,
    hold_fraction: f32,
}

impl ScrollingTextMotion {
    pub(in crate::app) fn for_widths(text_width: f32, viewport_width: f32) -> Option<Self> {
        if !text_width.is_finite()
            || !viewport_width.is_finite()
            || viewport_width <= 0.0
            || text_width <= viewport_width
        {
            return None;
        }
        let travel = text_width + TEXT_GAP;
        let seconds = (travel / SCROLL_SPEED + INITIAL_HOLD).max(MINIMUM_PERIOD);
        let period = Duration::try_from_secs_f32(seconds).ok()?;
        Some(Self {
            gap: TEXT_GAP,
            travel,
            period,
            hold_fraction: (INITIAL_HOLD / seconds).clamp(0.0, 0.8),
        })
    }

    pub(in crate::app) fn progress(self, delta: f32) -> f32 {
        if !delta.is_finite() || delta <= self.hold_fraction {
            0.0
        } else {
            ((delta - self.hold_fraction) / (1.0 - self.hold_fraction)).clamp(0.0, 1.0)
        }
    }
}

#[cfg(test)]
mod tests;
