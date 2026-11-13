//! Shared validation and playback-driven progress for all word timing formats.

use super::{LyricSegment, WordTimedLine};

impl WordTimedLine {
    pub fn is_valid(&self) -> bool {
        if self.text.trim().is_empty()
            || self.text.len() > 16_384
            || self.segments.is_empty()
            || self.segments.len() > 4096
            || self.end_ms.is_some_and(|end| end < self.start_ms)
        {
            return false;
        }
        let mut byte_end = 0;
        let mut time_end = self.start_ms;
        for segment in &self.segments {
            if segment.text_range.start < byte_end
                || segment.text_range.is_empty()
                || self.text.get(segment.text_range.clone()).is_none()
                || segment.start_ms < time_end
                || segment.end_ms.is_some_and(|end| end < segment.start_ms)
                || self.end_ms.is_some_and(|end| {
                    segment.start_ms > end || segment.end_ms.is_some_and(|stop| stop > end)
                })
            {
                return false;
            }
            byte_end = segment.text_range.end;
            time_end = segment.end_ms.unwrap_or(segment.start_ms);
        }
        true
    }
}

impl LyricSegment {
    pub fn progress(&self, position_ms: u64) -> f32 {
        if position_ms < self.start_ms {
            return 0.0;
        }
        match self.end_ms {
            Some(end) if end > self.start_ms => ((position_ms - self.start_ms) as f64
                / (end - self.start_ms) as f64)
                .clamp(0.0, 1.0) as f32,
            _ => 1.0,
        }
    }
}

#[cfg(test)]
mod tests;
