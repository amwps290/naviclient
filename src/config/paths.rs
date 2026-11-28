//! Platform storage directories and stable server namespace keys.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use super::cache_files::require_cache_directory;

use crate::models::Config;
use crate::source::{ProviderKind, SourceId};

pub(super) fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// 配置目录（会话、响度、流派缓存都放在这里）；不可用时退回当前目录。
pub(crate) fn config_dir() -> PathBuf {
    project_config_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// 单个非 OpenSubsonic 音乐源的持久化数据目录。
pub fn source_data_dir(source: &SourceId) -> PathBuf {
    config_dir().join("sources").join(source.cache_namespace())
}

fn project_config_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("rs", "navidrome", "navidrome-client")
        .map(|dirs| dirs.config_dir().to_path_buf())
}

/// 服务器作用域键：用于给会话/响度/流派缓存分命名空间。
///
/// 不同服务器的 song/album/cover id 会重名，混用会互相污染（例如 A 的响度
/// 补偿套到 B 的同名曲目上），因此这些缓存必须按服务器隔离。
/// 只保留 ASCII 字母数字与 `-`/`_`，避免出现路径分隔符。
pub fn server_scope(server_id: Option<&str>) -> String {
    source_scope(&SourceId::new(
        ProviderKind::OpenSubsonic,
        server_id.unwrap_or_default(),
    ))
}

/// New providers hash their full identity; OpenSubsonic keeps its original
/// storage paths so existing sessions, loudness and caches remain readable.
pub fn source_scope(source: &SourceId) -> String {
    if source.provider != ProviderKind::OpenSubsonic {
        return source.cache_namespace();
    }
    let scope: String = source
        .profile_id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .collect::<String>()
        .to_lowercase();
    if scope.is_empty() {
        "default".to_string()
    } else {
        scope
    }
}

fn default_audio_cache_dir() -> PathBuf {
    if let Some(project_dirs) =
        directories::ProjectDirs::from("rs", "navidrome", "navidrome-client")
    {
        project_dirs.cache_dir().join("audio")
    } else {
        PathBuf::from("navidrome-cache").join("audio")
    }
}

const CUSTOM_CACHE_DIRECTORY: &str = "navidrome-client-cache";
const CACHE_OWNER_FILE: &str = ".navidrome-client-owner";
const CACHE_OWNER: &[u8] = b"navidrome-client cache v1\n";

pub fn audio_cache_dir(config: &Config) -> PathBuf {
    config
        .cache_dir
        .as_ref()
        .map(|parent| parent.join(CUSTOM_CACHE_DIRECTORY))
        .unwrap_or_else(default_audio_cache_dir)
}

/// Claim a new child directory, but never adopt a pre-existing unmarked one.
/// Legacy files directly in the selected parent are not moved or removed.
pub fn prepare_audio_cache_dir(config: &Config) -> io::Result<PathBuf> {
    let directory = audio_cache_dir(config);
    if let Some(parent) = &config.cache_dir {
        fs::create_dir_all(parent)?;
        match fs::create_dir(&directory) {
            Ok(()) => fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join(CACHE_OWNER_FILE))?
                .write_all(CACHE_OWNER)?,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    } else {
        fs::create_dir_all(&directory)?;
    }
    verify_audio_cache_dir(config, &directory)?;
    Ok(directory)
}

pub(super) fn verify_audio_cache_dir(config: &Config, directory: &Path) -> io::Result<()> {
    require_cache_directory(directory)?;
    if config.cache_dir.is_none() {
        return Ok(());
    }
    let marker = directory.join(CACHE_OWNER_FILE);
    if fs::symlink_metadata(&marker).is_ok_and(|metadata| {
        metadata.file_type().is_file() && metadata.len() == CACHE_OWNER.len() as u64
    }) {
        let mut contents = Vec::new();
        fs::File::open(marker)?
            .take(CACHE_OWNER.len() as u64 + 1)
            .read_to_end(&mut contents)?;
        if contents == CACHE_OWNER {
            return Ok(());
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "refusing to use an unmanaged cache directory: {}",
            directory.display()
        ),
    ))
}

/// 封面磁盘缓存目录（与音频缓存同体系，切换缓存目录即切换命名空间）。
pub fn cover_cache_dir(config: &Config) -> PathBuf {
    audio_cache_dir(config).join("covers")
}

/// 受保护的离线音频存储目录（独立于自动 LRU 音频缓存，不会被缓存限额回收）。
pub fn offline_audio_dir(config: &Config) -> PathBuf {
    audio_cache_dir(config).join("offline")
}

pub fn log_path() -> PathBuf {
    if let Some(project_dirs) =
        directories::ProjectDirs::from("rs", "navidrome", "navidrome-client")
    {
        project_dirs
            .data_local_dir()
            .join("logs")
            .join("navidrome-client.log")
    } else {
        PathBuf::from("navidrome-client.log")
    }
}

#[cfg(test)]
mod tests;
