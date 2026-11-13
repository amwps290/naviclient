//! Bounded log snapshots for the in-app viewer and playback stall diagnostics.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::time::{Duration, Instant};

use crate::audio::PlaybackState;

pub(crate) const MAX_LOG_ENTRIES: usize = 1000;
pub(crate) const MAX_READ_BYTES: u64 = 512 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LogFilter {
    #[default]
    All,
    WarningsAndErrors,
    Playback,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Level {
    Error,
    Warn,
    Other,
}

#[derive(Debug, PartialEq, Eq)]
struct LogEntry {
    text: String,
    level: Level,
    playback: bool,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct LogTail {
    entries: Vec<LogEntry>,
    pub(crate) truncated: bool,
}

impl LogTail {
    pub(crate) fn chronological_text(&self) -> String {
        self.entries
            .iter()
            .map(|entry| entry.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(crate) fn filtered_text(&self, filter: LogFilter, search: &str) -> (String, usize) {
        let search = search.trim().to_lowercase();
        let entries = self.entries.iter().rev().filter(|entry| {
            (match filter {
                LogFilter::All => true,
                LogFilter::WarningsAndErrors => matches!(entry.level, Level::Error | Level::Warn),
                LogFilter::Playback => entry.playback,
            }) && (search.is_empty() || entry.text.to_lowercase().contains(&search))
        });
        let mut text = String::new();
        let mut count = 0;
        for entry in entries {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&entry.text);
            count += 1;
        }
        (text, count)
    }
}

pub(crate) fn read_recent_logs(path: &Path) -> io::Result<LogTail> {
    let mut file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(LogTail::default()),
        Err(error) => return Err(error),
    };
    let len = file.metadata()?.len();
    let start = len.saturating_sub(MAX_READ_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.take(len - start).read_to_end(&mut bytes)?;
    // A byte-limited read can start inside a UTF-8 character or log record.
    // Discard the incomplete first line; the remaining text is decoded safely.
    let bytes = if start > 0 {
        bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|newline| &bytes[newline + 1..])
            .unwrap_or_default()
    } else {
        &bytes
    };
    Ok(parse_tail(&String::from_utf8_lossy(bytes), start > 0))
}

pub(crate) fn parse_tail(text: &str, mut truncated: bool) -> LogTail {
    let mut entries: Vec<LogEntry> = Vec::new();
    for line in text.lines() {
        let header = line.strip_prefix('[').and_then(|line| line.split_once(']'));
        let level =
            header.and_then(
                |(header, _)| match header.split_ascii_whitespace().nth(1)? {
                    "ERROR" => Some(Level::Error),
                    "WARN" => Some(Level::Warn),
                    "INFO" | "DEBUG" | "TRACE" => Some(Level::Other),
                    _ => None,
                },
            );
        let line = sanitize_urls(line);
        if let Some(entry) = entries.last_mut().filter(|_| level.is_none()) {
            entry.text.push('\n');
            entry.text.push_str(&line);
        } else {
            entries.push(LogEntry {
                playback: line.contains("navidrome_client::audio")
                    || line.contains("navidrome_client::app::playback")
                    || line.contains("navidrome_client::diagnostics"),
                text: line,
                level: level.unwrap_or(Level::Other),
            });
        }
    }
    if entries.len() > MAX_LOG_ENTRIES {
        entries.drain(..entries.len() - MAX_LOG_ENTRIES);
        truncated = true;
    }
    LogTail { entries, truncated }
}

/// Remove URL credentials, queries and fragments before displaying or copying.
/// This also covers old logs written before stream URL redaction was added.
pub(crate) fn sanitize_urls(text: &str) -> String {
    let mut result = String::new();
    let mut remaining = text;
    loop {
        let start = [remaining.find("https://"), remaining.find("http://")]
            .into_iter()
            .flatten()
            .min();
        let Some(start) = start else {
            result.push_str(remaining);
            break;
        };
        result.push_str(&remaining[..start]);
        remaining = &remaining[start..];
        let end = remaining
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')'))
            .unwrap_or(remaining.len());
        if let Ok(mut url) = url::Url::parse(&remaining[..end]) {
            let _ = url.set_username("");
            let _ = url.set_password(None);
            url.set_query(None);
            url.set_fragment(None);
            result.push_str(url.as_str());
        } else {
            result.push_str("<URL>");
        }
        remaining = &remaining[end..];
    }
    result
}

/// Emits evidence independently of the audio callback, which may have stopped.
/// Repeated stalled snapshots are rate limited, and recovery is logged once.
#[derive(Default)]
pub(crate) struct PlaybackDiagnostics {
    last_stall_log: Option<Instant>,
    command_serial: u64,
}

impl PlaybackDiagnostics {
    pub(crate) fn observe(&mut self, playback: &PlaybackState, now: Instant) {
        if self.command_serial != playback.command_serial {
            self.last_stall_log = None;
            self.command_serial = playback.command_serial;
        }
        let Some(stall) = playback
            .stall
            .filter(|_| playback.active && !playback.paused)
        else {
            if self.last_stall_log.take().is_some() {
                log::info!(
                    "playback stall cleared; position_ms={} active={} paused={}",
                    playback.position.as_millis(),
                    playback.active,
                    playback.paused
                );
            }
            return;
        };
        if self.should_log(stall, now) {
            log::warn!(
                "playback stalled; stall_ms={} position_ms={} buffered_ms={} fully_buffered={} track_attached={} worker_closed={} output={:?} sample_rate={} channels={}",
                stall.as_millis(), playback.position.as_millis(), playback.buffered.as_millis(),
                playback.fully_buffered, playback.output_connected, playback.worker_closed,
                playback.output_name, playback.output_sample_rate, playback.output_channels
            );
        }
    }

    fn should_log(&mut self, stall: Duration, now: Instant) -> bool {
        if stall < Duration::from_secs(10)
            || self
                .last_stall_log
                .is_some_and(|last| now.duration_since(last) < Duration::from_secs(30))
        {
            return false;
        }
        self.last_stall_log = Some(now);
        true
    }
}

#[cfg(test)]
mod tests;
