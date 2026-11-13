use super::*;
use crate::diagnostics::LogFilter;

#[test]
fn disabling_file_output_preserves_memory_and_reenabling_appends() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("logs/client.log");
    let mut output = LogOutput::new(path.clone());
    output.write_record(b"[now INFO navidrome_client] memory only\n");
    assert!(!path.exists());
    output.set_file_enabled(true).unwrap();
    output.write_record(b"[now WARN navidrome_client] first saved\n");
    output.set_file_enabled(false).unwrap();
    output.write_record(b"[now ERROR navidrome_client] unsaved error\n");
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("first saved"));
    assert!(!text.contains("unsaved error"));
    assert!(!text.contains("memory only"));
    assert!(output
        .snapshot()
        .filtered_text(LogFilter::All, "")
        .0
        .contains("unsaved error"));
    output.set_file_enabled(true).unwrap();
    output.write_record(b"[now INFO navidrome_client] saved again\n");
    let text = fs::read_to_string(path).unwrap();
    assert!(text.contains("first saved"));
    assert!(text.contains("saved again"));
    assert!(!text.contains("unsaved error"));
}

#[test]
fn file_creation_failure_keeps_the_viewer_available() {
    let directory = tempfile::tempdir().unwrap();
    let blocker = directory.path().join("not-a-directory");
    fs::write(&blocker, "blocker").unwrap();
    let mut output = LogOutput::new(blocker.join("client.log"));
    assert!(output.set_file_enabled(true).is_err());
    assert!(output.file_error.is_some());
    output.write_record(b"[now ERROR navidrome_client] device lost\n");
    assert!(output
        .snapshot()
        .filtered_text(LogFilter::WarningsAndErrors, "device")
        .0
        .contains("device lost"));
}

#[test]
fn memory_history_is_bounded_and_retains_latest_unicode_entries() {
    let directory = tempfile::tempdir().unwrap();
    let mut output = LogOutput::new(directory.path().join("client.log"));
    let line = format!("[now INFO navidrome_client] {}\n", "旧日志".repeat(100));
    for _ in 0..1000 {
        output.write_record(line.as_bytes());
    }
    output.write_record("[now ERROR navidrome_client] 最新设备错误\n".as_bytes());
    assert!(output.memory_bytes <= MAX_READ_BYTES as usize);
    assert!(output.snapshot().truncated);
    assert!(output
        .snapshot()
        .filtered_text(LogFilter::All, "")
        .0
        .starts_with("[now ERROR navidrome_client] 最新设备错误"));
}

#[test]
fn a_full_log_rotates_on_the_next_record_without_losing_memory_history() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("client.log");
    let mut output = LogOutput::new(path.clone());
    output.set_file_enabled(true).unwrap();
    OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(MAX_FILE_BYTES)
        .unwrap();
    output.file_bytes = MAX_FILE_BYTES;
    output.write_record(b"[now ERROR navidrome_client] after rotation\n");
    assert_eq!(
        fs::metadata(path.with_extension("log.old")).unwrap().len(),
        MAX_FILE_BYTES
    );
    assert!(fs::read_to_string(path).unwrap().contains("after rotation"));
    assert!(output
        .snapshot()
        .filtered_text(LogFilter::All, "")
        .0
        .contains("after rotation"));
}

#[test]
fn file_output_defaults_on_for_old_configs_and_off_survives_roundtrip() {
    let legacy: crate::models::Config = serde_json::from_str("{}").unwrap();
    assert!(legacy.log_to_file);
    let config = crate::models::Config {
        log_to_file: false,
        ..Default::default()
    };
    let restored: crate::models::Config =
        serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
    assert!(!restored.log_to_file);
}
