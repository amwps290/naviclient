use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use md5::{Digest, Md5};

use crate::models::{Config, Genre, PlayModes, PlaybackMode, PlaybackSession};

fn config_path() -> PathBuf {
    if let Some(project_dirs) =
        directories::ProjectDirs::from("rs", "navidrome", "navidrome-client")
    {
        project_dirs.config_dir().join("config.json")
    } else {
        PathBuf::from("navidrome-client.json")
    }
}

fn session_path() -> PathBuf {
    if let Some(project_dirs) =
        directories::ProjectDirs::from("rs", "navidrome", "navidrome-client")
    {
        project_dirs.config_dir().join("session.json")
    } else {
        PathBuf::from("navidrome-session.json")
    }
}

fn loudness_path() -> PathBuf {
    if let Some(project_dirs) =
        directories::ProjectDirs::from("rs", "navidrome", "navidrome-client")
    {
        project_dirs.config_dir().join("loudness.json")
    } else {
        PathBuf::from("navidrome-loudness.json")
    }
}

fn genres_cache_path() -> PathBuf {
    if let Some(project_dirs) =
        directories::ProjectDirs::from("rs", "navidrome", "navidrome-client")
    {
        project_dirs.config_dir().join("genres.json")
    } else {
        PathBuf::from("navidrome-genres.json")
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

/// 由封面 id 派生缓存文件名：id 直接拼路径会有注入/非法字符风险，
/// 统一取 md5 十六进制。
pub fn cover_cache_file_name(cover_id: &str) -> String {
    format!("{:x}.cover", Md5::digest(cover_id.as_bytes()))
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

pub fn load() -> Config {
    let path = config_path();
    let mut config: Config = fs::read_to_string(&path)
        .ok()
        .and_then(|text| {
            let mut value: serde_json::Value = serde_json::from_str(&text).ok()?;
            migrate_legacy_playback_mode(&mut value);
            serde_json::from_value(value).ok()
        })
        .unwrap_or_default();
    config.volume = if config.volume.is_finite() {
        config.volume.clamp(0.0, 1.0)
    } else {
        Config::default().volume
    };
    config
}

/// 将旧版单枚举 `playback_mode` 迁移为二维 `playback_modes`
/// （随机开关 + 循环模式）。已有 `playback_modes` 或字段无法解析时不覆盖。
fn migrate_legacy_playback_mode(value: &mut serde_json::Value) {
    if value.get("playback_modes").is_some() {
        return;
    }
    let Some(legacy) = value
        .get("playback_mode")
        .cloned()
        .and_then(|value| serde_json::from_value::<PlaybackMode>(value).ok())
    else {
        return;
    };
    if let Ok(modes) = serde_json::to_value(PlayModes::from_legacy(legacy)) {
        value["playback_modes"] = modes;
    }
}

pub fn save(config: &Config) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create config directory")?;
    }
    let text = serde_json::to_string_pretty(config)?;
    fs::write(path, text).context("failed to write config")
}

/// 加载本地测量的歌曲响度缓存（song_id → (rms_db, peak)）；文件缺失或损坏时安全回退。
pub fn load_loudness() -> HashMap<String, (f32, f32)> {
    let path = loudness_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_loudness(cache: &HashMap<String, (f32, f32)>) -> Result<()> {
    let path = loudness_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create config directory")?;
    }
    let text = serde_json::to_string_pretty(cache)?;
    fs::write(path, text).context("failed to write loudness cache")
}

/// 加载播放会话；文件缺失或损坏时安全回退为 None。
/// 旧会话的 `playback_mode` 字段同样迁移为二维 `playback_modes`。
pub fn load_session() -> Option<PlaybackSession> {
    let text = fs::read_to_string(session_path()).ok()?;
    let mut value: serde_json::Value = serde_json::from_str(&text).ok()?;
    migrate_legacy_playback_mode(&mut value);
    serde_json::from_value(value).ok()
}

pub fn save_session(session: &PlaybackSession) -> Result<()> {
    let path = session_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create config directory")?;
    }
    let text = serde_json::to_string_pretty(session)?;
    fs::write(path, text).context("failed to write session")
}

/// 读取流派缓存（`getGenres` 在服务端要对全库聚合统计，大曲库下响应慢）。
/// 命中时先渲染旧数据、后台刷新后覆盖；文件缺失或损坏时安全回退为 None。
pub fn load_genres_cache() -> Option<Vec<Genre>> {
    read_genres_at(&genres_cache_path())
}

/// 保存最新流派列表，供下次启动即时展示。
pub fn save_genres_cache(genres: &[Genre]) -> Result<()> {
    write_genres_at(&genres_cache_path(), genres)
}

fn read_genres_at(path: &Path) -> Option<Vec<Genre>> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

fn write_genres_at(path: &Path, genres: &[Genre]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create config directory")?;
    }
    let text = serde_json::to_string_pretty(genres)?;
    fs::write(path, text).context("failed to write genres cache")
}

/// 读取封面磁盘缓存；未命中返回 None。
/// 网络请求固定使用 500px 档位，缓存键只需封面 id。
pub fn read_cover_cache(config: &Config, cover_id: &str) -> Option<Vec<u8>> {
    read_cover_at(&cover_cache_dir(config).join(cover_cache_file_name(cover_id)))
}

/// 下载成功后写入封面磁盘缓存。
pub fn write_cover_cache(config: &Config, cover_id: &str, bytes: &[u8]) {
    write_cover_at(
        &cover_cache_dir(config).join(cover_cache_file_name(cover_id)),
        bytes,
    )
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
mod tests {
    use super::*;
    use crate::models::Genre;

    fn temp_path(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "navidrome-client-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn cover_cache_file_name_is_deterministic_and_path_safe() {
        let a = cover_cache_file_name("al-123");
        let b = cover_cache_file_name("al-123");
        assert_eq!(a, b);
        assert!(a.ends_with(".cover"));
        // 任何 id（含路径分隔符等危险字符）都映射为纯十六进制名。
        assert_eq!(
            cover_cache_file_name("../evil/id"),
            cover_cache_file_name("../evil/id")
        );
        assert!(!cover_cache_file_name("../evil/id").contains('/'));
        assert_ne!(cover_cache_file_name("al-1"), cover_cache_file_name("al-2"));
    }

    #[test]
    fn cover_cache_round_trips_and_missing_files_miss() {
        let dir = std::env::temp_dir().join(format!("navidrome-cover-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = Config {
            cache_dir: Some(dir.clone()),
            ..Config::default()
        };

        assert!(read_cover_cache(&config, "al-404").is_none());
        write_cover_cache(&config, "al-1", b"png-bytes");
        assert_eq!(
            read_cover_cache(&config, "al-1").as_deref(),
            Some(b"png-bytes".as_slice())
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn cover_cache_prune_removes_oldest_files_beyond_limit() {
        let dir =
            std::env::temp_dir().join(format!("navidrome-cover-prune-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // 三个文件共 300 字节；上限 200：最旧的 two.bin 应被删到回到限额内。
        std::fs::write(dir.join("oldest.cover"), vec![0u8; 150]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.join("middle.cover"), vec![0u8; 100]).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(dir.join("newest.cover"), vec![0u8; 50]).unwrap();

        let removed = prune_cover_cache_at(&dir, 200);
        assert_eq!(removed, 150);
        assert!(!dir.join("oldest.cover").exists());
        assert!(dir.join("middle.cover").exists());
        assert!(dir.join("newest.cover").exists());

        // 限额内不删除。
        assert_eq!(prune_cover_cache_at(&dir, 200), 0);
        // 目录不存在时静默返回 0。
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(prune_cover_cache_at(&dir, 200), 0);
    }

    #[test]
    fn legacy_playback_mode_field_migrates_to_playback_modes() {
        let mut value: serde_json::Value =
            serde_json::from_str(r#"{"server_url":"http://x","playback_mode":"repeat_one"}"#)
                .unwrap();
        migrate_legacy_playback_mode(&mut value);
        assert_eq!(value["playback_modes"]["shuffle"], serde_json::json!(false));
        assert_eq!(value["playback_modes"]["repeat"], "one");

        let mut value: serde_json::Value =
            serde_json::from_str(r#"{"playback_mode":"shuffle"}"#).unwrap();
        migrate_legacy_playback_mode(&mut value);
        assert_eq!(value["playback_modes"]["shuffle"], serde_json::json!(true));
        assert_eq!(value["playback_modes"]["repeat"], "off");
    }

    #[test]
    fn existing_playback_modes_win_over_legacy_field() {
        let mut value: serde_json::Value = serde_json::from_str(
            r#"{"playback_modes":{"shuffle":true,"repeat":"all"},"playback_mode":"sequential"}"#,
        )
        .unwrap();
        migrate_legacy_playback_mode(&mut value);
        assert_eq!(value["playback_modes"]["repeat"], "all");
        assert_eq!(value["playback_modes"]["shuffle"], serde_json::json!(true));
    }

    #[test]
    fn genres_cache_round_trips_through_disk() {
        let path = temp_path("genres.json");
        let genres = vec![
            Genre {
                value: "Rock".to_string(),
                song_count: Some(120),
                album_count: Some(12),
            },
            Genre {
                value: "Jazz".to_string(),
                song_count: None,
                album_count: None,
            },
        ];

        write_genres_at(&path, &genres).expect("cache write should succeed");
        let restored = read_genres_at(&path).expect("cache read should succeed");
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].value, "Rock");
        assert_eq!(restored[0].song_count, Some(120));

        // 缺失与损坏文件都必须安全回退，不能 panic。
        std::fs::remove_file(&path).unwrap();
        assert!(read_genres_at(&path).is_none());
        std::fs::write(&path, "{not valid json").unwrap();
        assert!(read_genres_at(&path).is_none());
        std::fs::remove_file(&path).unwrap();
    }
}
