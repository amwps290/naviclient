//! Pure, tolerant LRC parsing. File offset is applied once, before song offset.

use super::adjusted_time;
use crate::models::{LyricLine, Lyrics};

pub(crate) fn plain(text: &str) -> Lyrics {
    Lyrics {
        lines: text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|text| LyricLine {
                text: text.to_string(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

fn timestamp(tag: &str) -> Option<u64> {
    let (minutes, rest) = tag.split_once(':')?;
    let minutes = minutes.parse::<u64>().ok()?;
    let (seconds, fraction) = rest.split_once('.').unwrap_or((rest, ""));
    let seconds = seconds.parse::<u64>().ok().filter(|s| *s < 60)?;
    if !fraction.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let millis = if fraction.is_empty() {
        0
    } else {
        let digits = &fraction[..fraction.len().min(3)];
        digits.parse::<u64>().ok()? * 10u64.pow(3 - digits.len() as u32)
    };
    minutes
        .checked_mul(60_000)?
        .checked_add(seconds * 1000)?
        .checked_add(millis)
}

pub(crate) fn parse(text: &str) -> Lyrics {
    let mut lyrics = Lyrics::default();
    let mut offset = 0i64;
    for raw in text.trim_start_matches('\u{feff}').lines().take(10_000) {
        let mut rest = raw.trim();
        let mut times = Vec::new();
        while let Some(tagged) = rest.strip_prefix('[') {
            let Some((tag, tail)) = tagged.split_once(']') else {
                break;
            };
            if let Some(time) = timestamp(tag) {
                if times.len() < 32 {
                    times.push(time);
                }
            } else if let Some((key, value)) = tag.split_once(':') {
                match key.to_ascii_lowercase().as_str() {
                    "offset" => offset = value.trim().parse().unwrap_or(0),
                    "ar" => lyrics.display_artist = Some(value.trim().to_string()),
                    "ti" => lyrics.display_title = Some(value.trim().to_string()),
                    _ => {}
                }
            }
            rest = tail;
        }
        let text: String = rest.trim().chars().take(4096).collect();
        if !text.is_empty() {
            for time in times {
                if lyrics.lines.len() < 10_000 {
                    lyrics.lines.push(LyricLine {
                        start_ms: Some(time),
                        text: text.clone(),
                        translation: None,
                    });
                }
            }
        }
    }
    for line in &mut lyrics.lines {
        line.start_ms = line.start_ms.map(|ms| adjusted_time(ms, offset));
    }
    lyrics.lines.sort_by_key(|l| l.start_ms);
    lyrics
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tags_precision_and_offsets_are_tolerant() {
        let l = parse("\u{feff}[ar:歌手]\n[ti:歌曲]\n[00:02.3][00:01.025] hello\n[00:03.1234]world\n[offset:-1100]\n[00:00.2]early\n[bad]ignored\n[00:04]\n[99999999999999999:22]overflow");
        assert_eq!(l.display_artist.as_deref(), Some("歌手"));
        assert_eq!(
            l.lines
                .iter()
                .map(|l| l.start_ms.unwrap())
                .collect::<Vec<_>>(),
            vec![0, 0, 1200, 2023]
        );
        assert_eq!(l.lines[2].text, "hello");
        assert!(!parse("[00:xx]broken").is_synced());
        assert_eq!(plain("\n 中文 \n\n English ").lines.len(), 2);
    }
}
