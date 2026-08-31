use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::models::{Config, PlaybackSession};

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
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    config.volume = if config.volume.is_finite() {
        config.volume.clamp(0.0, 1.0)
    } else {
        Config::default().volume
    };
    config
}

pub fn save(config: &Config) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create config directory")?;
    }
    let text = serde_json::to_string_pretty(config)?;
    fs::write(path, text).context("failed to write config")
}

/// 加载播放会话；文件缺失或损坏时安全回退为 None。
pub fn load_session() -> Option<PlaybackSession> {
    let path = session_path();
    fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
}

pub fn save_session(session: &PlaybackSession) -> Result<()> {
    let path = session_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create config directory")?;
    }
    let text = serde_json::to_string_pretty(session)?;
    fs::write(path, text).context("failed to write session")
}
