use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

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
