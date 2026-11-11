//! Atomic JSON replacement and tolerant cache reads.

use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};

/// Serialize first, then atomically replace the destination from a temporary
/// file in the same directory. A crash can therefore leave either the old or
/// the new complete JSON document, but never a truncated destination file.
pub(crate) fn write_json_atomic<T>(path: &Path, value: &T, label: &str) -> Result<()>
where
    T: serde::Serialize + ?Sized,
{
    let text = serde_json::to_string_pretty(value)
        .with_context(|| format!("failed to serialize {label}"))?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create directory for {label}"))?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temporary file for {label}"))?;
    temporary
        .write_all(text.as_bytes())
        .with_context(|| format!("failed to write temporary {label}"))?;
    temporary
        .as_file()
        .sync_all()
        .with_context(|| format!("failed to flush temporary {label}"))?;
    temporary
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to replace {label}"))?;
    Ok(())
}

pub(super) fn read_json_at<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

#[cfg(test)]
mod tests;
