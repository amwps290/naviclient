//! Runtime-switchable file logging with a bounded, always-on in-memory history.

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::Context as _;

use crate::diagnostics::{parse_tail, read_recent_logs, LogTail, MAX_READ_BYTES};

const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
static OUTPUT: OnceLock<Arc<Mutex<LogOutput>>> = OnceLock::new();

struct LogOutput {
    path: PathBuf,
    file: Option<File>,
    file_bytes: u64,
    file_error: Option<String>,
    memory: VecDeque<String>,
    memory_bytes: usize,
    truncated: bool,
}

impl LogOutput {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            file: None,
            file_bytes: 0,
            file_error: None,
            memory: VecDeque::new(),
            memory_bytes: 0,
            truncated: false,
        }
    }

    fn set_file_enabled(&mut self, enabled: bool) -> io::Result<()> {
        if !enabled {
            self.file = None;
            self.file_error = None;
            return Ok(());
        }
        if self.file.is_some() {
            return Ok(());
        }
        match self.open_file() {
            Ok(()) => {
                self.file_error = None;
                Ok(())
            }
            Err(error) => {
                self.file_error = Some(error.to_string());
                Err(error)
            }
        }
    }

    fn open_file(&mut self) -> io::Result<()> {
        if let Some(directory) = self.path.parent() {
            fs::create_dir_all(directory)?;
        }
        if self
            .path
            .metadata()
            .is_ok_and(|metadata| metadata.len() >= MAX_FILE_BYTES)
        {
            let old_path = self.path.with_extension("log.old");
            match fs::remove_file(&old_path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
            fs::rename(&self.path, old_path)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.file_bytes = file.metadata()?.len();
        self.file = Some(file);
        Ok(())
    }

    fn remember(&mut self, text: &str) {
        for line in text.split_inclusive('\n') {
            self.memory.push_back(line.to_string());
            self.memory_bytes += line.len();
            while self.memory_bytes > MAX_READ_BYTES as usize {
                if let Some(line) = self.memory.pop_front() {
                    self.memory_bytes -= line.len();
                    self.truncated = true;
                }
            }
        }
    }

    fn write_record(&mut self, buffer: &[u8]) {
        self.remember(&String::from_utf8_lossy(buffer));
        if self.file.is_none() {
            return;
        }
        let result = (|| {
            if self.file_bytes >= MAX_FILE_BYTES {
                self.file = None; // Windows requires the handle closed before renaming.
                self.open_file()?;
            }
            if let Some(file) = &mut self.file {
                file.write_all(buffer)?;
                self.file_bytes += buffer.len() as u64;
            }
            Ok::<_, io::Error>(())
        })();
        if let Err(error) = result {
            self.record_file_error(error);
        }
    }

    fn record_file_error(&mut self, error: io::Error) {
        self.file = None;
        self.file_error = Some(error.to_string());
        // Do not call log! while holding the output mutex: that would recurse.
        self.remember(&format!(
            "[{} ERROR navidrome_client::logging] file logging failed: {error}\n",
            chrono::Utc::now().to_rfc3339()
        ));
    }

    fn snapshot(&self) -> LogTail {
        let text = self.memory.iter().map(String::as_str).collect::<String>();
        parse_tail(&text, self.truncated)
    }
}

struct LogWriter {
    output: Arc<Mutex<LogOutput>>,
    mirror_to_stderr: bool,
}

impl Write for LogWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.output
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .write_record(buffer);
        if self.mirror_to_stderr {
            let _ = io::stderr().write_all(buffer);
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut output = self
            .output
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let error = output.file.as_mut().and_then(|file| file.flush().err());
        if let Some(error) = error {
            output.record_file_error(error);
        }
        if self.mirror_to_stderr {
            let _ = io::stderr().flush();
        }
        Ok(())
    }
}

pub(crate) fn init(file_enabled: bool) -> anyhow::Result<PathBuf> {
    let path = crate::config::log_path();
    let mut output = LogOutput::new(path.clone());
    // Seed the viewer with recent history before startup rotation, so restarting
    // after a failure does not hide the events immediately preceding the restart.
    if let Ok(tail) = read_recent_logs(&path) {
        output.truncated = tail.truncated;
        let text = tail.chronological_text();
        if !text.is_empty() {
            output.remember(&format!("{text}\n"));
        }
    }
    let initial_error = output.set_file_enabled(file_enabled).err();
    let output = Arc::new(Mutex::new(output));
    let writer = LogWriter {
        output: output.clone(),
        mirror_to_stderr: cfg!(debug_assertions),
    };
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("warn,navidrome_client=debug"),
    )
    .format_timestamp_millis()
    .target(env_logger::Target::Pipe(Box::new(writer)))
    .try_init()
    .context("failed to initialize logger")?;
    let _ = OUTPUT.set(output);
    if let Some(error) = initial_error {
        log::error!("file logging unavailable; keeping logs in memory: {error}");
    }
    Ok(path)
}

pub(crate) fn set_file_enabled(enabled: bool) -> io::Result<()> {
    let output = OUTPUT
        .get()
        .ok_or_else(|| io::Error::other("logger not initialized"))?;
    output
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .set_file_enabled(enabled)
}

pub(crate) fn snapshot() -> LogTail {
    OUTPUT
        .get()
        .map(|output| {
            output
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .snapshot()
        })
        .unwrap_or_default()
}

pub(crate) fn file_error() -> Option<String> {
    OUTPUT.get().and_then(|output| {
        output
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .file_error
            .clone()
    })
}

pub(crate) fn open_directory() -> io::Result<PathBuf> {
    let path = crate::config::log_path();
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::other("log directory is unavailable"))?;
    fs::create_dir_all(directory)?;
    Ok(directory.to_path_buf())
}

#[cfg(test)]
mod tests;
