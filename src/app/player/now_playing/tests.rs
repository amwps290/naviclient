use super::*;
use crate::models::{LyricLine, Lyrics};

#[test]
fn opacity_falls_with_distance_and_never_below_the_floor() {
    let values = [1, 2, 3, 4, 10, usize::MAX].map(lyric_opacity);
    assert!(values.windows(2).all(|pair| pair[0] >= pair[1]));
    assert_eq!(lyric_opacity(usize::MAX), 0.55);
    assert!(values.iter().all(|value| *value >= 0.55));
}

#[test]
fn display_focus_holds_the_previous_line_between_vocal_cues() {
    let lyrics =
        crate::lyrics::yrc::parse("[1000,1000](1000,1000,0)first\n[4000,1000](4000,1000,0)next");
    for (position, expected) in [
        (999, None),
        (1000, Some(0)),
        (1999, Some(0)),
        (2000, Some(0)),
        (3999, Some(0)),
        (4000, Some(1)),
        (5000, Some(1)),
        (9000, Some(1)),
        // Pausing in the gap, then seeking backward, must not retain a later row.
        (2500, Some(0)),
        (2500, Some(0)),
        (0, None),
    ] {
        assert_eq!(
            lyrics.display_line_index(Duration::from_millis(position)),
            expected,
            "position={position}"
        );
    }
    assert_eq!(lyrics.active_line_index(Duration::from_millis(2500)), None);
    let first = &lyrics.lines[0].word_timing.as_ref().unwrap().segments[0];
    let next = &lyrics.lines[1].word_timing.as_ref().unwrap().segments[0];
    assert_eq!(first.progress(2500), 1.0);
    assert_eq!(next.progress(2500), 0.0);
}

#[test]
fn display_focus_skips_empty_cues_and_untimed_metadata() {
    let lyrics = Lyrics {
        lines: vec![
            LyricLine {
                start_ms: Some(1000),
                end_ms: Some(2000),
                text: "first".into(),
                ..Default::default()
            },
            LyricLine {
                start_ms: Some(2000),
                text: "  \t".into(),
                ..Default::default()
            },
            LyricLine {
                text: "credits".into(),
                ..Default::default()
            },
            LyricLine {
                start_ms: Some(5000),
                text: "next".into(),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    assert_eq!(lyrics.display_line_index(Duration::from_millis(999)), None);
    assert_eq!(
        lyrics.display_line_index(Duration::from_millis(2000)),
        Some(0)
    );
    assert_eq!(
        lyrics.display_line_index(Duration::from_millis(4999)),
        Some(0)
    );
    assert_eq!(
        lyrics.display_line_index(Duration::from_millis(5000)),
        Some(3)
    );
}

#[test]
fn display_focus_follows_adjusted_timestamps_and_plain_mode_has_no_focus() {
    let mut document = crate::lyrics::Document::native(crate::lyrics::yrc::parse(
        "[1000,1000](1000,1000,0)first\n[4000,1000](4000,1000,0)next",
    ));
    document.offset_ms = 500;
    let lyrics = document.display_lyrics();
    for (position, expected) in [
        (1499, None),
        (1500, Some(0)),
        (3000, Some(0)),
        (4499, Some(0)),
        (4500, Some(1)),
        (1499, None),
    ] {
        assert_eq!(
            lyrics.display_line_index(Duration::from_millis(position)),
            expected
        );
    }
    document.plain = true;
    assert_eq!(
        document
            .display_lyrics()
            .display_line_index(Duration::from_millis(9000)),
        None
    );
}
