//! Server-specific cover keys, raw response caching, and bounded disk retention.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use md5::{Digest, Md5};

use super::cache_files::{is_cache_digest, prune_cache_files, require_cache_directory};
use super::paths::{
    audio_cache_dir, cover_cache_dir, prepare_audio_cache_dir, verify_audio_cache_dir,
};
use super::server_scope;
use crate::models::Config;
use crate::source::{ProviderKind, SourceId};

/// 由服务器作用域 + 封面 id 派生缓存文件名。
///
/// id 直接拼路径会有注入/非法字符风险，统一取 md5 十六进制；同时把服务器
/// 作用域并入哈希，避免两台服务器用同一个 cover id 时互相看到对方的图。
pub fn cover_cache_file_name(server_id: Option<&str>, cover_id: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(server_scope(server_id).as_bytes());
    hasher.update(b"/");
    hasher.update(cover_id.as_bytes());
    format!("{:x}.cover", hasher.finalize())
}

/// 封面缓存的完整路径，通知图标等需要读原文件的场景直接复用。
#[cfg(test)]
pub fn cover_cache_path(config: &Config, server_id: Option<&str>, cover_id: &str) -> PathBuf {
    cover_cache_dir(config).join(cover_cache_file_name(server_id, cover_id))
}

pub fn source_cover_cache_path(config: &Config, source: &SourceId, cover_id: &str) -> PathBuf {
    let name = if source.provider == ProviderKind::OpenSubsonic {
        cover_cache_file_name(Some(&source.profile_id), cover_id)
    } else {
        let mut hash = Md5::new();
        hash.update(cover_id.as_bytes());
        format!("{}-{:x}.cover", source.cache_namespace(), hash.finalize())
    };
    cover_cache_dir(config).join(name)
}

pub fn read_source_cover_cache(
    config: &Config,
    source: &SourceId,
    cover_id: &str,
) -> Option<Vec<u8>> {
    read_cover_at(config, &source_cover_cache_path(config, source, cover_id))
}

pub fn write_source_cover_cache(config: &Config, source: &SourceId, cover_id: &str, bytes: &[u8]) {
    write_cover_at(
        config,
        &source_cover_cache_path(config, source, cover_id),
        bytes,
    );
}

/// 读取缓存的原封字节；文件缺失或读取失败按未命中处理。
fn read_cover_at(config: &Config, path: &Path) -> Option<Vec<u8>> {
    verify_audio_cache_dir(config, &audio_cache_dir(config)).ok()?;
    require_cache_directory(&cover_cache_dir(config)).ok()?;
    fs::read(path).ok()
}

/// 写入原封字节；失败只记日志不影响播放。
fn write_cover_at(config: &Config, path: &Path, bytes: &[u8]) {
    let result = (|| -> io::Result<()> {
        prepare_audio_cache_dir(config)?;
        let directory = cover_cache_dir(config);
        fs::create_dir_all(&directory)?;
        require_cache_directory(&directory)?;
        fs::write(path, bytes)
    })();
    if let Err(error) = result {
        log::warn!("failed to write cover cache {path:?}: {error}");
    }
}

/// 缓存总量上限；超出时从最旧开始删除。
const COVER_CACHE_MAX_BYTES: u64 = 256 * 1024 * 1024;

fn is_cover_cache_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".cover") else {
        return false;
    };
    if let Some((source, cover)) = stem
        .strip_prefix("source-")
        .and_then(|stem| stem.split_once('-'))
    {
        is_cache_digest(source) && is_cache_digest(cover)
    } else {
        is_cache_digest(stem)
    }
}

fn prune_cover_cache_at(config: &Config, max_bytes: u64) -> io::Result<u64> {
    verify_audio_cache_dir(config, &audio_cache_dir(config))?;
    prune_cache_files(&cover_cache_dir(config), max_bytes, is_cover_cache_name)
}

/// 读取封面磁盘缓存；未命中返回 None。
/// 同一封面 id 只保留一份原始响应字节；请求尺寸调整不影响旧缓存兼容性。
#[cfg(test)]
pub fn read_cover_cache(
    config: &Config,
    server_id: Option<&str>,
    cover_id: &str,
) -> Option<Vec<u8>> {
    read_cover_at(config, &cover_cache_path(config, server_id, cover_id))
}

/// 下载成功后写入封面磁盘缓存。
#[cfg(test)]
pub fn write_cover_cache(config: &Config, server_id: Option<&str>, cover_id: &str, bytes: &[u8]) {
    write_cover_at(
        config,
        &cover_cache_path(config, server_id, cover_id),
        bytes,
    );
}

/// 启动时清理超限的封面缓存；返回删除的字节数。
pub fn prune_cover_cache(config: &Config) -> u64 {
    let dir = cover_cache_dir(config);
    match prune_cover_cache_at(config, COVER_CACHE_MAX_BYTES) {
        Ok(removed) => {
            if removed > 0 {
                log::info!("pruned {removed} bytes of cover cache under {dir:?}");
            }
            removed
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => {
            log::warn!("skipped cover cache cleanup under {dir:?}: {error}");
            0
        }
    }
}

#[cfg(test)]
mod tests;
