//! Shared cleanup for application-owned disk caches.

use std::fs;
use std::io;
use std::path::Path;
use std::time::SystemTime;

pub(crate) fn require_cache_directory(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.file_type().is_dir() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "cache directory is not a regular directory: {}",
                path.display()
            ),
        ))
    }
}

/// Remove the oldest recognized files until the owned bytes fit the limit.
/// Do not count or delete other entries, including directories and symlinks.
pub(crate) fn prune_cache_files(
    directory: &Path,
    limit: u64,
    owns_name: impl Fn(&str) -> bool,
) -> io::Result<u64> {
    require_cache_directory(directory)?;
    let mut files = Vec::new();
    let mut total = 0_u64;
    for entry in fs::read_dir(directory)?.flatten() {
        if !entry.file_name().to_str().is_some_and(&owns_name) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        // DirEntry::metadata does not follow symbolic links.
        if !metadata.file_type().is_file() {
            continue;
        }
        let len = metadata.len();
        total = total.saturating_add(len);
        files.push((
            metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            entry.path(),
            len,
        ));
    }
    if total <= limit {
        return Ok(0);
    }

    files.sort_unstable();
    let mut removed = 0_u64;
    for (_, path, len) in files {
        if total <= limit {
            break;
        }
        if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_file())
            && fs::remove_file(&path).is_ok()
        {
            total = total.saturating_sub(len);
            removed = removed.saturating_add(len);
        }
    }
    Ok(removed)
}

pub(crate) fn is_cache_digest(name: &str) -> bool {
    name.len() == 32
        && name
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}
