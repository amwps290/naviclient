use super::*;

fn words() -> WordTimedLine {
    WordTimedLine {
        text: "你 love love".into(),
        start_ms: 1000,
        end_ms: Some(3000),
        segments: vec![
            LyricSegment {
                text_range: 0..3,
                start_ms: 1000,
                end_ms: Some(1500),
            },
            LyricSegment {
                text_range: 9..13,
                start_ms: 2000,
                end_ms: Some(3000),
            },
        ],
    }
}

#[test]
fn ranges_preserve_utf8_and_identify_the_second_repeated_word() {
    let mut words = words();
    assert!(words.is_valid());
    assert_eq!(&words.text[words.segments[1].text_range.clone()], "love");
    words.segments[0].text_range.end = 1;
    assert!(!words.is_valid());
}

#[test]
fn invalid_ranges_and_overlapping_times_are_rejected() {
    let mut words = words();
    words.segments[1].start_ms = 1499;
    assert!(!words.is_valid());
    words = super::tests::words();
    words.segments[1].text_range.end = usize::MAX;
    assert!(!words.is_valid());
}

#[test]
fn progress_handles_seeks_gaps_start_only_and_instantaneous_cues() {
    let mut segment = words().segments.remove(0);
    assert_eq!(segment.progress(999), 0.0);
    assert_eq!(segment.progress(1250), 0.5);
    assert_eq!(segment.progress(9000), 1.0);
    assert_eq!(segment.progress(1000), 0.0);
    segment.end_ms = Some(1000);
    assert_eq!(segment.progress(1000), 1.0);
    segment.end_ms = None;
    assert_eq!(segment.progress(1000), 1.0);
}
