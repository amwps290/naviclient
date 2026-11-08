//! Platform storage directories and stable server namespace keys.

use std::path::PathBuf;

use crate::models::Config;
use crate::source::{ProviderKind, SourceId};

pub(super) fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// 配置目录（会话、响度、流派缓存都放在这里）；不可用时退回当前目录。
pub(super) fn config_dir() -> PathBuf {
    project_config_dir().unwrap_or_else(|| PathBuf::from("."))
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

pub fn default_audio_cache_dir() -> PathBuf {
    if let Some(project_dirs) =
        directories::ProjectDirs::from("rs", "navidrome", "navidrome-client")
    {
        project_dirs.cache_dir().join("audio")
    } else {
        PathBuf::from("navidrome-cache").join("audio")
    }
}

pub fn audio_cache_dir(config: &Config) -> PathBuf {
    config
        .cache_dir
        .clone()
        .unwrap_or_else(default_audio_cache_dir)
}

/// 封面磁盘缓存目录（与音频缓存同体系，切换缓存目录即切换命名空间）。
pub fn cover_cache_dir(config: &Config) -> PathBuf {
    audio_cache_dir(config).join("covers")
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
