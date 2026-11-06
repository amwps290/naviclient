//! Global preference loading, legacy upgrade, and atomic persistence.

use std::fs;
use std::path::Path;

use anyhow::Result;

use super::json::write_json_atomic;
use super::migration::migrate_legacy_playback_mode;
use super::paths::config_path;
use crate::models::Config;

pub fn load() -> Config {
    read_config_at(&config_path())
}

fn read_config_at(path: &Path) -> Config {
    let mut config: Config = match fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(mut value) => {
                migrate_legacy_playback_mode(&mut value);
                serde_json::from_value(value).unwrap_or_else(|error| {
                    log::warn!("invalid config at {}: {error}", path.display());
                    Config::default()
                })
            }
            Err(error) => {
                log::warn!("invalid config JSON at {}: {error}", path.display());
                Config::default()
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Config::default(),
        Err(error) => {
            log::warn!("failed to read config at {}: {error}", path.display());
            Config::default()
        }
    };
    config.volume = if config.volume.is_finite() {
        config.volume.clamp(0.0, 1.0)
    } else {
        Config::default().volume
    };
    config.equalizer.clamp_gains();
    // 旧配置只有单个 server_url/username/password：收编成服务器列表的一条，
    // 并保证 active_server 指向它。
    config.sync_active_profile();
    config
}

pub fn save(config: &Config) -> Result<()> {
    write_json_atomic(&config_path(), config, "config")
}

#[cfg(test)]
mod tests;
