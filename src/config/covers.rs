//! Server-specific cover keys, raw response caching, and bounded disk retention.

use std::fs;
use std::path::{Path, PathBuf};

use md5::{Digest, Md5};

use super::{cover_cache_dir, server_scope};
use crate::models::Config;

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
pub fn cover_cache_path(config: &Config, server_id: Option<&str>, cover_id: &str) -> PathBuf {
    cover_cache_dir(config).join(super::cover_cache_file_name(server_id, cover_id))
}

/// 读取缓存的原封字节；文件缺失或读取失败按未命中处理。
fn read_cover_at(path: &Path) -> Option<Vec<u8>> {
    fs::read(path).ok()
}

/// 写入原封字节；失败只记日志不影响播放。
fn write_cover_at(path: &Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Err(error) = fs::write(path, bytes) {
        log::warn!("failed to write cover cache {:?}: {error}", path);
    }
}

/// 缓存总量上限；超出时从最旧开始删除。
const COVER_CACHE_MAX_BYTES: u64 = 256 * 1024 * 1024;

/// 清理封面缓存：总量超过上限时按修改时间从最旧开始删除，
/// 返回删除的字节数供日志记录。目录不存在时静默返回 0。
fn prune_cover_cache_at(dir: &Path, max_bytes: u64) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut files = Vec::new();
    let mut total: u64 = 0;
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let size = meta.len();
        let Ok(modified) = meta.modified() else {
            continue;
        };
        total += size;
        files.push((modified, size, entry.path()));
    }
    if total <= max_bytes {
        return 0;
    }
    // 最旧的在前；逐个删除直到回到限额内。
    files.sort_by_key(|(modified, _, _)| *modified);
    let mut removed = 0;
    for (_, size, path) in files {
        if total <= max_bytes {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
            removed += size;
        }
    }
    removed
}

/// 读取封面磁盘缓存；未命中返回 None。
/// 同一封面 id 只保留一份原始响应字节；请求尺寸调整不影响旧缓存兼容性。
pub fn read_cover_cache(
    config: &Config,
    server_id: Option<&str>,
    cover_id: &str,
) -> Option<Vec<u8>> {
    read_cover_at(&cover_cache_path(config, server_id, cover_id))
}

/// 下载成功后写入封面磁盘缓存。
pub fn write_cover_cache(config: &Config, server_id: Option<&str>, cover_id: &str, bytes: &[u8]) {
    write_cover_at(&cover_cache_path(config, server_id, cover_id), bytes);
}

/// 启动时清理超限的封面缓存；返回删除的字节数。
pub fn prune_cover_cache(config: &Config) -> u64 {
    let dir = cover_cache_dir(config);
    let removed = prune_cover_cache_at(&dir, COVER_CACHE_MAX_BYTES);
    if removed > 0 {
        log::info!("pruned {removed} bytes of cover cache under {dir:?}");
    }
    removed
}

#[cfg(test)]
mod tests;
