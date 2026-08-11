use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use crate::models::Config;

fn config_path() -> PathBuf {
    if let Some(project_dirs) =
        directories::ProjectDirs::from("rs", "navidrome", "navidrome-client")
    {
        project_dirs.config_dir().join("config.json")
    } else {
        PathBuf::from("navidrome-client.json")
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
    fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(config: &Config) -> Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create config directory")?;
    }
    let text = serde_json::to_string_pretty(config)?;
    fs::write(path, text).context("failed to write config")
}
