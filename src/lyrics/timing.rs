//! Personal word boundary corrections, tied to an immutable lyrics version.

use super::Document;
use crate::models::Lyrics;
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TimingEdits {
    pub fingerprint: String,
    pub words: Vec<WordTimingEdit>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WordTimingEdit {
    pub line: usize,
    pub segment: usize,
    pub start_ms: u64,
    pub end_ms: Option<u64>,
}

impl Document {
    pub fn timing_fingerprint(&self) -> String {
        let rows: Vec<_> = self
            .lyrics
            .lines
            .iter()
            .map(|line| {
                let words = line.word_timing.as_ref().map(|words| {
                    let segments: Vec<_> = words
                        .segments
                        .iter()
                        .map(|s| (s.text_range.start, s.text_range.end, s.start_ms, s.end_ms))
                        .collect();
                    (&words.text, words.start_ms, words.end_ms, segments)
                });
                (&line.text, line.start_ms, line.end_ms, words)
            })
            .collect();
        format!(
            "{:x}",
            Md5::digest(serde_json::to_vec(&rows).expect("lyrics timing fingerprint"))
        )
    }

    pub fn restore_timing_edits(&mut self, edits: Option<TimingEdits>) {
        self.timing_edits = edits.filter(|edits| self.apply_timing_edits(edits).is_some());
    }

    pub fn edited_lyrics(&self) -> Lyrics {
        self.timing_edits
            .as_ref()
            .and_then(|edits| self.apply_timing_edits(edits))
            .unwrap_or_else(|| self.lyrics.clone())
    }

    fn apply_timing_edits(&self, edits: &TimingEdits) -> Option<Lyrics> {
        if edits.fingerprint != self.timing_fingerprint() {
            return None;
        }
        let mut lyrics = self.lyrics.clone();
        let mut seen = std::collections::HashSet::new();
        for edit in &edits.words {
            if !seen.insert((edit.line, edit.segment)) {
                return None;
            }
            let segment = lyrics
                .lines
                .get_mut(edit.line)?
                .word_timing
                .as_mut()?
                .segments
                .get_mut(edit.segment)?;
            segment.start_ms = edit.start_ms;
            segment.end_ms = edit.end_ms;
        }
        for line in &mut lyrics.lines {
            if let Some(words) = &mut line.word_timing {
                // An earlier first word also needs an earlier row/header boundary.
                let first = words.segments.first()?.start_ms;
                words.start_ms = words.start_ms.min(first);
                line.start_ms = line.start_ms.map(|start| start.min(first));
                if !words.is_valid() {
                    return None;
                }
            }
        }
        Some(lyrics)
    }

    /// Move the onset boundary, keeping the selected word's end. For adjacent
    /// words the preceding word shares this boundary, so continuous YRC timing
    /// remains editable without introducing overlaps.
    pub fn set_word_start(&mut self, line: usize, segment: usize, requested_ms: u64) -> bool {
        let mut lyrics = self.edited_lyrics();
        let Some(words) = lyrics
            .lines
            .get_mut(line)
            .and_then(|line| line.word_timing.as_mut())
        else {
            return false;
        };
        let Some(current) = words.segments.get(segment) else {
            return false;
        };
        let old_start = current.start_ms;
        let lower = segment
            .checked_sub(1)
            .map_or(0, |i| words.segments[i].start_ms);
        let upper = current
            .end_ms
            .or_else(|| words.segments.get(segment + 1).map(|s| s.start_ms))
            .or(words.end_ms)
            .unwrap_or(u64::MAX);
        let start = requested_ms.clamp(lower, upper.max(lower));
        if start == old_start {
            return false;
        }
        words.segments[segment].start_ms = start;
        if let Some(previous) = segment.checked_sub(1).map(|i| &mut words.segments[i]) {
            if let Some(end) = previous.end_ms {
                previous.end_ms = Some(if end == old_start {
                    start
                } else {
                    end.min(start)
                });
            }
        }
        let mut edits = Vec::new();
        for (line, (edited, original)) in lyrics.lines.iter().zip(&self.lyrics.lines).enumerate() {
            if let (Some(edited), Some(original)) = (&edited.word_timing, &original.word_timing) {
                for (segment, (a, b)) in edited.segments.iter().zip(&original.segments).enumerate()
                {
                    if (a.start_ms, a.end_ms) != (b.start_ms, b.end_ms) {
                        edits.push(WordTimingEdit {
                            line,
                            segment,
                            start_ms: a.start_ms,
                            end_ms: a.end_ms,
                        });
                    }
                }
            }
        }
        let edits = TimingEdits {
            fingerprint: self.timing_fingerprint(),
            words: edits,
        };
        if self.apply_timing_edits(&edits).is_none() {
            return false;
        }
        self.timing_edits = (!edits.words.is_empty()).then_some(edits);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn document() -> Document {
        Document::native(crate::lyrics::yrc::parse(
            "[1000,3000](1000,1000,0)你(2000,1000,0)好(3000,1000,0)呀",
        ))
    }

    #[test]
    fn continuous_words_share_the_corrected_boundary_and_offset_is_applied_once() {
        let mut doc = document();
        assert!(doc.set_word_start(0, 1, 2300));
        doc.offset_ms = 500;
        let display = doc.display_lyrics();
        let words = display.lines[0].word_timing.as_ref().unwrap();
        assert_eq!(words.segments[0].end_ms, Some(2800));
        assert_eq!(words.segments[1].start_ms, 2800);
        assert_eq!(words.segments[1].end_ms, Some(3500));
        assert!(words.is_valid());
        assert_eq!(doc.lyrics, document().lyrics);
    }

    #[test]
    fn first_word_can_move_earlier_and_invalid_or_stale_edits_are_ignored() {
        let mut doc = document();
        assert!(doc.set_word_start(0, 0, 500));
        let edited = doc.edited_lyrics();
        assert_eq!(edited.lines[0].start_ms, Some(500));
        assert!(edited.lines[0].word_timing.as_ref().unwrap().is_valid());
        let edits = doc.timing_edits.clone();
        let mut other = Document::native(crate::lyrics::yrc::parse("[1000,1000](1000,1000,0)别"));
        other.restore_timing_edits(edits);
        assert!(other.timing_edits.is_none());
        let mut bad = doc.timing_edits.clone().unwrap();
        bad.words[0].start_ms = 9000;
        doc.restore_timing_edits(Some(bad));
        assert!(doc.timing_edits.is_none());
    }

    #[test]
    fn boundary_clamps_and_returning_to_source_removes_corrections() {
        let mut doc = document();
        assert!(doc.set_word_start(0, 1, 0));
        assert_eq!(
            doc.edited_lyrics().lines[0]
                .word_timing
                .as_ref()
                .unwrap()
                .segments[1]
                .start_ms,
            1000
        );
        assert!(doc.set_word_start(0, 1, 2000));
        assert!(doc.timing_edits.is_none());
    }
}
