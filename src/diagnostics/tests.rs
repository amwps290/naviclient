use super::*;
use std::io::Write;

#[test]
fn filters_keep_multiline_errors_and_put_newest_entries_first() {
    let tail = parse_tail(
        "[2026-11-13T14:12:58Z INFO  navidrome_client::audio::decoder] ready\r\n\
         [2026-11-13T14:12:59Z ERROR navidrome_client::audio::output] device lost\r\n\
         caused by: driver stopped\r\n\
         [2026-11-13T14:13:00Z WARN  navidrome_client::config] save failed\r\n",
        false,
    );
    let (text, count) = tail.filtered_text(LogFilter::WarningsAndErrors, "");
    assert_eq!(count, 2);
    assert!(text.starts_with("[2026-11-13T14:13:00Z"));
    assert!(text.contains("device lost\ncaused by: driver stopped"));
    assert!(!text.contains("ready"));
    let (text, count) = tail.filtered_text(LogFilter::Playback, " DRIVER ");
    assert_eq!(count, 1);
    assert!(text.contains("device lost"));
    assert_eq!(tail.filtered_text(LogFilter::Playback, "save").1, 0);
}

#[test]
fn viewer_strips_signed_queries_and_url_credentials() {
    let text = sanitize_urls(
        "endpoint=\"https://user:password@example.com/audio.m4a?token=secret&sign=private#fragment\" error=(http://example.com/stream?t=secret)",
    );
    assert_eq!(
        text,
        "endpoint=\"https://example.com/audio.m4a\" error=(http://example.com/stream)"
    );
    for secret in ["password", "token", "secret", "private", "fragment"] {
        assert!(!text.contains(secret));
    }
    let tail = parse_tail(
        "[now WARN navidrome_client] https://example.com/?password=secret",
        false,
    );
    assert!(!tail.filtered_text(LogFilter::All, "").0.contains("secret"));
}

#[test]
fn reading_large_log_is_bounded_and_preserves_recent_utf8() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(&vec![b'x'; MAX_READ_BYTES as usize + 10])
        .unwrap();
    writeln!(file, "\n[now ERROR navidrome_client::audio] 设备已失效").unwrap();
    file.flush().unwrap();
    let tail = read_recent_logs(file.path()).unwrap();
    assert!(tail.truncated);
    assert_eq!(tail.filtered_text(LogFilter::All, "").1, 1);
    assert!(tail
        .filtered_text(LogFilter::All, "")
        .0
        .contains("设备已失效"));
}

#[test]
fn only_the_most_recent_entries_are_retained() {
    let text = (0..MAX_LOG_ENTRIES + 20)
        .map(|i| format!("[now INFO navidrome_client] record {i}\n"))
        .collect::<String>();
    let tail = parse_tail(&text, false);
    assert!(tail.truncated);
    assert_eq!(tail.filtered_text(LogFilter::All, "").1, MAX_LOG_ENTRIES);
    assert!(tail
        .filtered_text(LogFilter::All, "")
        .0
        .starts_with("[now INFO navidrome_client] record 1019"));
    assert_eq!(tail.filtered_text(LogFilter::All, "record 0").1, 0);
}

#[test]
fn missing_log_is_an_empty_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    assert_eq!(
        read_recent_logs(&directory.path().join("missing.log")).unwrap(),
        LogTail::default()
    );
}

#[test]
fn stall_logs_are_delayed_rate_limited_and_reset_after_recovery() {
    let mut diagnostics = PlaybackDiagnostics::default();
    let start = Instant::now();
    assert!(!diagnostics.should_log(Duration::from_secs(9), start));
    assert!(diagnostics.should_log(Duration::from_secs(10), start));
    assert!(!diagnostics.should_log(Duration::from_secs(20), start + Duration::from_secs(10)));
    assert!(diagnostics.should_log(Duration::from_secs(40), start + Duration::from_secs(30)));
    diagnostics.observe(&PlaybackState::default(), start + Duration::from_secs(31));
    assert!(diagnostics.should_log(Duration::from_secs(10), start + Duration::from_secs(32)));
}
