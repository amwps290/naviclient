//! NetEase YRC: line and segment starts are absolute track milliseconds.

use crate::models::{LyricLine, LyricSegment, Lyrics, WordTimedLine};

fn numbers(text: &str) -> Option<Vec<u64>> {
    text.split(',').map(|part| part.parse().ok()).collect()
}

pub(crate) fn parse(text: &str) -> Lyrics {
    let mut lines = Vec::new();
    for raw in text.trim_start_matches('\u{feff}').lines().take(10_000) {
        let raw = raw.trim_start();
        let Some((header, content)) = raw.strip_prefix('[').and_then(|s| s.split_once(']')) else {
            continue;
        };
        let Some(times) = numbers(header).filter(|times| times.len() == 2) else {
            continue;
        };
        let start_ms = times[0];
        let end_ms = start_ms.checked_add(times[1]);
        if content.len() > 65_536 {
            continue;
        }
        let markers: Vec<_> = content
            .match_indices('(')
            .filter_map(|(index, _)| {
                let rest = &content[index..];
                if !rest
                    .as_bytes()
                    .get(1)
                    .is_some_and(|b| b.is_ascii_digit() || *b == b'-')
                {
                    return None;
                }
                let stop = rest.bytes().take(64).position(|b| b == b')')?;
                let tag = &rest[1..stop];
                if tag.split(',').count() != 3 {
                    return None;
                }
                let times = numbers(tag);
                Some((index, index + stop + 1, times))
            })
            .take(4097)
            .collect();
        let mut words = WordTimedLine {
            text: String::new(),
            start_ms,
            end_ms,
            segments: Vec::new(),
        };
        let mut valid = end_ms.is_some();
        if let Some(first) = markers.first() {
            words.text.push_str(&content[..first.0]);
        } else {
            words.text.push_str(content);
        }
        for (index, (_, text_start, times)) in markers.iter().enumerate() {
            let text_end = markers.get(index + 1).map_or(content.len(), |next| next.0);
            let range_start = words.text.len();
            words.text.push_str(&content[*text_start..text_end]);
            let Some(times) = times else {
                valid = false;
                continue;
            };
            let (start, duration) = (times[0], times[1]);
            let end = start.checked_add(duration);
            valid &= end.is_some();
            if range_start < words.text.len() {
                words.segments.push(LyricSegment {
                    text_range: range_start..words.text.len(),
                    start_ms: start,
                    end_ms: end,
                });
            }
        }
        if words.text.trim().is_empty() || words.text.len() > 16_384 {
            continue;
        }
        lines.push(LyricLine {
            start_ms: Some(start_ms),
            end_ms,
            text: words.text.clone(),
            word_timing: (valid && words.is_valid()).then_some(words),
            ..Default::default()
        });
    }
    lines.sort_by_key(|line| line.start_ms);
    Lyrics {
        lines,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests;
