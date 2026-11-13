use super::*;

#[test]
fn absolute_times_spaces_and_zero_duration_are_preserved() {
    let lyrics =
        parse("{\"metadata\":true}\n[1000,2000](1000,500,0)你 (1500,0,0)好(2000,1000,0) world");
    let line = &lyrics.lines[0];
    assert_eq!(line.text, "你 好 world");
    let words = line.word_timing.as_ref().unwrap();
    assert_eq!(words.segments[0].start_ms, 1000);
    assert_eq!(words.segments[2].end_ms, Some(3000));
    assert_eq!(&words.text[words.segments[1].text_range.clone()], "好");
}

#[test]
fn invalid_word_timing_falls_back_to_readable_line_text() {
    for input in [
        "[1000,2000](1000,600,0)你(1500,500,0)好",
        "[1000,2000](1000,18446744073709551615,0)你",
        "[1000,2000](900,500,0)你",
        "[1000,2000](1000,500,0)你(1500,bad,0)好",
    ] {
        let lyrics = parse(input);
        assert_eq!(lyrics.lines.len(), 1);
        assert!(lyrics.lines[0].word_timing.is_none());
        assert!(!lyrics.lines[0].text.contains("(1000,"));
    }
}

#[test]
fn metadata_blank_lines_and_literal_parentheses_do_not_break_words() {
    let lyrics = parse("\u{feff}{\"t\":0}\n\n[1000,1000](1000,1000,0)hello (world)\n[bad]ignored");
    assert_eq!(lyrics.lines.len(), 1);
    assert_eq!(lyrics.lines[0].text, "hello (world)");
    assert!(lyrics.has_word_timing());
}

#[test]
fn legacy_record_without_word_data_and_invalid_yrc_still_uses_lrc() {
    use crate::lyrics::provider::Record;
    let mut record: Record =
        serde_json::from_str(r#"{"id":1,"syncedLyrics":"[00:01]fallback","plainLyrics":null}"#)
            .unwrap();
    assert_eq!(record.lyrics().lines[0].text, "fallback");
    record.yrc_lyrics = Some("[1000,1000](900,300,0)broken".into());
    assert_eq!(record.lyrics().lines[0].text, "fallback");
    record.synced_lyrics = None;
    assert_eq!(record.lyrics().lines[0].text, "broken");
}

#[test]
fn document_offset_updates_all_times_once_and_plain_mode_removes_sync() {
    use crate::lyrics::Document;
    let mut document = Document::native(parse("[1000,1000](1000,1000,0)test"));
    document.offset_ms = -1200;
    let line = document.display_lyrics().lines.remove(0);
    assert_eq!(line.start_ms, Some(0));
    assert_eq!(line.end_ms, Some(800));
    let words = line.word_timing.unwrap();
    assert_eq!(words.start_ms, 0);
    assert_eq!(words.segments[0].start_ms, 0);
    assert_eq!(words.segments[0].end_ms, Some(800));
    assert_eq!(document.display_lyrics().lines[0].end_ms, Some(800));
    document.plain = true;
    let line = document.display_lyrics().lines.remove(0);
    assert!(line.start_ms.is_none() && line.end_ms.is_none() && line.word_timing.is_none());
}
