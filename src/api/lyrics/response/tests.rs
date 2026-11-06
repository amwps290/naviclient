use std::time::Duration;

use super::*;

fn test_lyric_line(start_ms: Option<u64>, text: &str) -> LyricLine {
    LyricLine {
        start_ms,
        text: text.to_string(),
        translation: None,
    }
}

#[test]
fn translations_pair_by_nearest_timestamp_within_tolerance() {
    let mut primary = vec![
        test_lyric_line(Some(0), "hello"),
        test_lyric_line(Some(5000), "world"),
    ];
    let candidate = vec![
        test_lyric_line(Some(100), "你好"),
        test_lyric_line(Some(5100), "世界"),
        test_lyric_line(Some(9000), "多余行"),
    ];
    pair_translations(&mut primary, &candidate);
    assert_eq!(primary[0].translation.as_deref(), Some("你好"));
    assert_eq!(primary[1].translation.as_deref(), Some("世界"));
}

#[test]
fn translations_reject_far_and_duplicate_and_empty_text() {
    let mut primary = vec![
        test_lyric_line(Some(0), "a"),
        test_lyric_line(Some(10_000), "b"),
        test_lyric_line(Some(20_000), "same"),
    ];
    let candidate = vec![
        test_lyric_line(Some(30_000), "超出容差"),
        test_lyric_line(Some(20_000), "same"),
        test_lyric_line(Some(10_000), ""),
    ];
    pair_translations(&mut primary, &candidate);
    assert_eq!(primary[0].translation, None);
    // 空文本不作为翻译。
    assert_eq!(primary[1].translation, None);
    // 与原文相同也不重复显示。
    assert_eq!(primary[2].translation, None);
}

#[test]
fn unsynced_translations_pair_by_line_index_only() {
    let mut primary = vec![test_lyric_line(None, "one"), test_lyric_line(None, "two")];
    let candidate = vec![test_lyric_line(None, "一"), test_lyric_line(None, "二")];
    pair_translations(&mut primary, &candidate);
    assert_eq!(primary[0].translation.as_deref(), Some("一"));
    assert_eq!(primary[1].translation.as_deref(), Some("二"));

    // 行数不同时不做行号对齐，宁缺毋滥。
    let mut primary = vec![test_lyric_line(None, "one")];
    pair_translations(&mut primary, &candidate);
    assert_eq!(primary[0].translation, None);
}

#[test]
fn structured_lyrics_translation_variant_is_extracted() {
    let body = serde_json::json!({
        "lyricsList": { "structuredLyrics": [
            {
                "lang": "eng",
                "synced": true,
                "line": [
                    {"start": 0, "value": "hello"},
                    {"start": 5000, "value": "world"},
                ]
            },
            {
                "lang": "zho",
                "synced": true,
                "line": [
                    {"start": 0, "value": "你好"},
                    {"start": 5000, "value": "世界"},
                ]
            },
        ]}
    });
    let lyrics = parse_structured_lyrics(&body).expect("structured lyrics should parse");
    assert_eq!(lyrics.lines[0].text, "hello");
    assert_eq!(lyrics.lines[0].translation.as_deref(), Some("你好"));
    assert_eq!(lyrics.lines[1].text, "world");
    assert_eq!(lyrics.lines[1].translation.as_deref(), Some("世界"));
}

#[test]
fn same_language_variants_do_not_become_translations() {
    let body = serde_json::json!({
        "lyricsList": { "structuredLyrics": [
            {
                "lang": "eng",
                "synced": true,
                "line": [{"start": 0, "value": "hello"}]
            },
            {
                "lang": "eng",
                "synced": false,
                "line": [{"value": "hello (alternate)"}]
            },
        ]}
    });
    let lyrics = parse_structured_lyrics(&body).expect("structured lyrics should parse");
    assert_eq!(lyrics.lines[0].text, "hello");
    assert_eq!(lyrics.lines[0].translation, None);
}

#[test]
fn parses_synced_structured_lyrics_with_offset() {
    let body = serde_json::json!({
        "lyricsList": {
            "structuredLyrics": [{
                "synced": true,
                "offset": -100,
                "line": [
                    {"start": 1000, "value": "First line"},
                    {"start": 2500, "value": "Second line"}
                ]
            }]
        }
    });

    let lyrics = parse_structured_lyrics(&body).expect("structured lyrics should parse");
    assert!(lyrics.is_synced());
    assert_eq!(lyrics.lines[0].start_ms, Some(900));
    assert_eq!(
        lyrics.active_line_index(Duration::from_millis(2_000)),
        Some(0)
    );
    assert_eq!(
        lyrics.active_line_index(Duration::from_millis(3_000)),
        Some(1)
    );
}

#[test]
fn structured_lyrics_drop_all_blank_rows() {
    let body = serde_json::json!({
        "lyricsList": {
            "structuredLyrics": [{
                "synced": true,
                "line": [
                    {"start": 0, "value": ""},
                    {"start": 1000, "value": " First line "},
                    {"start": 2000, "value": "   "},
                    {"start": 2500, "value": ""},
                    {"start": 3000, "value": "Second line"},
                    {"start": 4000, "value": ""}
                ]
            }]
        }
    });

    let lyrics = parse_structured_lyrics(&body).expect("structured lyrics should parse");
    assert_eq!(lyrics.lines.len(), 2);
    assert_eq!(lyrics.lines[0].text, "First line");
    assert_eq!(lyrics.lines[1].text, "Second line");
}

#[test]
fn structured_variant_ranking_counts_content_instead_of_blank_rows() {
    let body = serde_json::json!({
        "lyricsList": { "structuredLyrics": [
            {
                "lang": "zho",
                "synced": true,
                "line": [
                    {"start": 0, "value": "Only real line"},
                    {"start": 1000, "value": ""},
                    {"start": 2000, "value": ""},
                    {"start": 3000, "value": ""}
                ]
            },
            {
                "lang": "eng",
                "synced": true,
                "line": [
                    {"start": 0, "value": "First real line"},
                    {"start": 1000, "value": "Second real line"}
                ]
            }
        ]}
    });

    let lyrics = parse_structured_lyrics(&body).expect("structured lyrics should parse");
    assert_eq!(lyrics.lines.len(), 2);
    assert_eq!(lyrics.lines[0].text, "First real line");
    assert_eq!(lyrics.lines[1].text, "Second real line");
}

#[test]
fn parses_plain_lyrics_lines() {
    let body = serde_json::json!({
        "lyrics": {
            "artist": "Artist",
            "title": "Song",
            "value": " First line \n\n   \nSecond line"
        }
    });

    let lyrics = parse_plain_lyrics(&body).expect("plain lyrics should parse");
    assert!(!lyrics.is_synced());
    assert_eq!(lyrics.lines.len(), 2);
    assert_eq!(lyrics.lines[0].text, "First line");
    assert_eq!(lyrics.lines[1].text, "Second line");
}

#[test]
fn blank_synced_lines_never_receive_translation_text() {
    let mut primary = vec![
        test_lyric_line(Some(0), "original"),
        test_lyric_line(Some(5_000), ""),
    ];
    let candidate = vec![
        test_lyric_line(Some(0), "翻译"),
        test_lyric_line(Some(5_000), "不应写入间奏行"),
    ];

    pair_translations(&mut primary, &candidate);
    assert_eq!(primary[0].translation.as_deref(), Some("翻译"));
    assert_eq!(primary[1].translation, None);
}
