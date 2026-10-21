use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use md5::{Digest, Md5};

use crate::models::{Config, Genre, PlayModes, PlaybackMode, PlaybackSession};

fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// 配置目录（会话、响度、流派缓存都放在这里）；不可用时退回当前目录。
fn config_dir() -> PathBuf {
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
    let scope: String = server_id
        .unwrap_or_default()
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

fn scoped_path(stem: &str, server_id: Option<&str>) -> PathBuf {
    scoped_path_in(&config_dir(), stem, server_id)
}

/// 指定目录下的作用域缓存文件名（供测试与批量清理使用）。
fn scoped_path_in(dir: &Path, stem: &str, server_id: Option<&str>) -> PathBuf {
    dir.join(format!("{stem}-{}.json", server_scope(server_id)))
}

/// 旧版（单服务器）全局文件名，仅用于一次性迁移。
fn legacy_path(stem: &str) -> PathBuf {
    config_dir().join(format!("{stem}.json"))
}

fn session_path(server_id: Option<&str>) -> PathBuf {
    scoped_path("session", server_id)
}

fn loudness_path(server_id: Option<&str>) -> PathBuf {
    scoped_path("loudness", server_id)
}

fn genres_cache_path(server_id: Option<&str>) -> PathBuf {
    scoped_path("genres", server_id)
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
    cover_cache_dir(config).join(cover_cache_file_name(server_id, cover_id))
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
    // 旧配置只有单个 server_url/username/password：收编成服务器列表的一条，
    // 并保证 active_server 指向它。
    config.sync_active_profile();
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
pub fn load_loudness(server_id: Option<&str>) -> HashMap<String, (f32, f32)> {
    let path = loudness_path(server_id);
    if let Some(cache) = read_json_at(&path) {
        return cache;
    }
    // 旧版没有服务器标记的全局缓存：由第一台激活的服务器接管后删除，
    // 避免之后被另一台服务器复用（song id 会撞）。
    let legacy = legacy_path("loudness");
    let Some(cache) = read_json_at(&legacy) else {
        return HashMap::new();
    };
    if let Err(error) = save_loudness(server_id, &cache) {
        log::warn!("failed to migrate legacy loudness cache: {error:#}");
        return cache;
    }
    let _ = fs::remove_file(&legacy);
    log::info!("migrated legacy loudness cache into the active server scope");
    cache
}

pub fn save_loudness(server_id: Option<&str>, cache: &HashMap<String, (f32, f32)>) -> Result<()> {
    let path = loudness_path(server_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create config directory")?;
    }
    let text = serde_json::to_string_pretty(cache)?;
    fs::write(path, text).context("failed to write loudness cache")
}

/// 加载播放会话；文件缺失或损坏时安全回退为 None。
/// 旧会话的 `playback_mode` 字段同样迁移为二维 `playback_modes`。
pub fn load_session(server_id: Option<&str>, server_url: &str) -> Option<PlaybackSession> {
    if let Some(session) = read_session_at(&session_path(server_id)) {
        return Some(session);
    }
    // 旧版全局会话自带 server_url：只有与当前服务器匹配时才接管，
    // 否则会把 A 的队列带进 B。
    let legacy = legacy_path("session");
    let session = read_session_at(&legacy)?;
    if session.server_url.trim_end_matches('/') != server_url.trim_end_matches('/') {
        return None;
    }
    if let Err(error) = save_session(server_id, &session) {
        log::warn!("failed to migrate legacy playback session: {error:#}");
    }
    let _ = fs::remove_file(&legacy);
    log::info!("migrated legacy playback session into the active server scope");
    Some(session)
}

fn read_session_at(path: &Path) -> Option<PlaybackSession> {
    let text = fs::read_to_string(path).ok()?;
    let mut value: serde_json::Value = serde_json::from_str(&text).ok()?;
    migrate_legacy_playback_mode(&mut value);
    serde_json::from_value(value).ok()
}

fn read_json_at<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

pub fn save_session(server_id: Option<&str>, session: &PlaybackSession) -> Result<()> {
    let path = session_path(server_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create config directory")?;
    }
    let text = serde_json::to_string_pretty(session)?;
    fs::write(path, text).context("failed to write session")
}

/// 读取流派缓存（`getGenres` 在服务端要对全库聚合统计，大曲库下响应慢）。
/// 命中时先渲染旧数据、后台刷新后覆盖；文件缺失或损坏时安全回退为 None。
pub fn load_genres_cache(server_id: Option<&str>) -> Option<Vec<Genre>> {
    if let Some(genres) = read_genres_at(&genres_cache_path(server_id)) {
        return Some(genres);
    }
    // 旧版全局缓存同样只接管一次（流派统计按库不同，不能跨服务器复用）。
    let legacy = legacy_path("genres");
    let genres = read_genres_at(&legacy)?;
    if let Err(error) = save_genres_cache(server_id, &genres) {
        log::warn!("failed to migrate legacy genres cache: {error:#}");
        return Some(genres);
    }
    let _ = fs::remove_file(&legacy);
    Some(genres)
}

/// 保存最新流派列表，供下次启动即时展示。
pub fn save_genres_cache(server_id: Option<&str>, genres: &[Genre]) -> Result<()> {
    write_genres_at(&genres_cache_path(server_id), genres)
}

/// 删除某台服务器的作用域缓存文件（播放会话 / 响度 / 流派），
/// 返回删除的文件数。
///
/// 封面与音频缓存文件名由内容 id 哈希而来，无法按服务器枚举，
/// 继续交给总量上限的旧文件清理；这里只删可以确定性定位的三个文件。
pub fn remove_server_cache(server_id: &str) -> usize {
    remove_server_cache_at(&config_dir(), server_id)
}

fn remove_server_cache_at(dir: &Path, server_id: &str) -> usize {
    ["session", "loudness", "genres"]
        .iter()
        .map(|stem| scoped_path_in(dir, stem, Some(server_id)))
        .filter(|path| fs::remove_file(path).is_ok())
        .count()
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
        let a = cover_cache_file_name(Some("srv-a"), "al-123");
        let b = cover_cache_file_name(Some("srv-a"), "al-123");
        assert_eq!(a, b);
        assert!(a.ends_with(".cover"));
        // 任何 id（含路径分隔符等危险字符）都映射为纯十六进制名。
        assert_eq!(
            cover_cache_file_name(Some("srv-a"), "../evil/id"),
            cover_cache_file_name(Some("srv-a"), "../evil/id")
        );
        assert!(!cover_cache_file_name(Some("srv-a"), "../evil/id").contains('/'));
        assert_ne!(
            cover_cache_file_name(Some("srv-a"), "al-1"),
            cover_cache_file_name(Some("srv-a"), "al-2")
        );
    }

    #[test]
    fn cover_cache_does_not_cross_servers_with_the_same_id() {
        // 两台服务器都可能返回 id "al-1"，但封面内容不同：缓存键必须区分。
        assert_ne!(
            cover_cache_file_name(Some("srv-a"), "al-1"),
            cover_cache_file_name(Some("srv-b"), "al-1")
        );
    }

    #[test]
    fn server_scope_is_path_safe_and_stable() {
        assert_eq!(server_scope(Some("a1b2c3d4e5f6")), "a1b2c3d4e5f6");
        assert_eq!(server_scope(Some("AB-CD_ef")), "ab-cd_ef");
        // 空 id 与含非法字符的 id 都退回 default，绝不产生路径分隔符。
        assert_eq!(server_scope(None), "default");
        assert_eq!(server_scope(Some("")), "default");
        assert_eq!(server_scope(Some("../../etc")), "etc");
    }

    #[test]
    fn scoped_cache_paths_differ_per_server() {
        assert_ne!(session_path(Some("srv-a")), session_path(Some("srv-b")));
        assert_ne!(loudness_path(Some("srv-a")), loudness_path(Some("srv-b")));
        assert_ne!(
            genres_cache_path(Some("srv-a")),
            genres_cache_path(Some("srv-b"))
        );
        assert!(scoped_path("session", Some("srv-a"))
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("srv-a"));
    }

    #[test]
    fn cover_cache_round_trips_and_missing_files_miss() {
        let dir = std::env::temp_dir().join(format!("navidrome-cover-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = Config {
            cache_dir: Some(dir.clone()),
            ..Config::default()
        };

        assert!(read_cover_cache(&config, None, "al-404").is_none());
        write_cover_cache(&config, Some("srv-a"), "al-1", b"png-bytes");
        assert_eq!(
            read_cover_cache(&config, Some("srv-a"), "al-1").as_deref(),
            Some(b"png-bytes".as_slice())
        );
        // 另一台服务器读不到这份缓存（同 id 不同内容）。
        assert!(read_cover_cache(&config, Some("srv-b"), "al-1").is_none());

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
    fn remove_server_cache_deletes_only_that_servers_scoped_files() {
        let dir = std::env::temp_dir().join(format!(
            "navidrome-remove-cache-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for stem in ["session", "loudness", "genres"] {
            std::fs::write(scoped_path_in(&dir, stem, Some("srv-a")), "a").unwrap();
            std::fs::write(scoped_path_in(&dir, stem, Some("srv-b")), "b").unwrap();
        }

        assert_eq!(remove_server_cache_at(&dir, "srv-a"), 3);
        for stem in ["session", "loudness", "genres"] {
            assert!(!scoped_path_in(&dir, stem, Some("srv-a")).exists());
            assert!(
                scoped_path_in(&dir, stem, Some("srv-b")).exists(),
                "另一台服务器的缓存不能被误删"
            );
        }

        // 不存在的服务器（无文件可删）与空目录都安全返回 0。
        assert_eq!(remove_server_cache_at(&dir, "srv-404"), 0);
        let empty = std::env::temp_dir().join(format!(
            "navidrome-remove-cache-empty-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&empty);
        assert_eq!(remove_server_cache_at(&empty, "srv-a"), 0);

        std::fs::remove_dir_all(&dir).unwrap();
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
