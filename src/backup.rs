//! One-click backup and migration bundle for personal preferences, equalizer presets,
//! lyrics selections/timing offsets, cross-source local playlists, and listening history.
//!
//! Credentials (server passwords, WebDAV passwords, Bilibili session keys, and proxy
//! userinfo) are always stripped on export and preserved locally on restore.

#[cfg(test)]
mod tests;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::listening_stats::log_store::ListeningLogEntry;
use crate::local_playlists::LocalPlaylist;
use crate::lyrics::cache::LyricsPreferencesBackup;
use crate::models::{Config, EqSettings, ServerProfile};
use crate::source::SourceProfile;

pub const BACKUP_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestoreMode {
    Merge,
    Overwrite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BackupSelection {
    pub config: bool,
    pub equalizer: bool,
    pub lyrics_preferences: bool,
    pub local_playlists: bool,
    pub listening_log: bool,
}

impl Default for BackupSelection {
    fn default() -> Self {
        Self {
            config: true,
            equalizer: true,
            lyrics_preferences: true,
            local_playlists: true,
            listening_log: true,
        }
    }
}

impl BackupSelection {
    pub fn any(self) -> bool {
        self.config
            || self.equalizer
            || self.lyrics_preferences
            || self.local_playlists
            || self.listening_log
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupBundle {
    pub schema_version: u32,
    pub exported_at: u64,
    #[serde(default)]
    pub app_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<Config>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equalizer: Option<EqSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lyrics_preferences: Option<LyricsPreferencesBackup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_playlists: Option<Vec<LocalPlaylist>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listening_log: Option<Vec<ListeningLogEntry>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RestoreSummary {
    pub config_restored: bool,
    pub equalizer_restored: bool,
    pub lyrics_restored: usize,
    pub playlists_restored: usize,
    pub listening_entries_restored: usize,
    pub safety_snapshot: Option<PathBuf>,
}

/// Strip all plaintext passwords, OS keyring flags, Bilibili credential references,
/// and proxy userinfo so backups can be safely stored in cloud storage or shared across machines.
pub fn sanitize_config_for_backup(config: &Config) -> Config {
    let mut sanitized = config.clone();
    sanitized.password.clear();
    for server in &mut sanitized.servers {
        server.password.clear();
        server.password_in_keyring = false;
    }
    for source in &mut sanitized.sources {
        match source {
            SourceProfile::WebDav {
                password,
                password_in_keyring,
                ..
            } => {
                password.clear();
                *password_in_keyring = false;
            }
            SourceProfile::Bilibili {
                account_id,
                credential_key,
                ..
            } => {
                *account_id = None;
                *credential_key = None;
            }
            SourceProfile::OpenSubsonic { .. } | SourceProfile::LocalFolder { .. } => {}
        }
    }
    for proxy in &mut sanitized.network.proxies {
        proxy.url = strip_url_userinfo(&proxy.url);
    }
    sanitized
}

/// Apply an imported config onto the current config while preserving existing local
/// credentials for matching servers/sources.
pub fn apply_config_backup(current: &Config, incoming: &Config, mode: RestoreMode) -> Config {
    let incoming = sanitize_config_for_backup(incoming);
    let mut next = match mode {
        RestoreMode::Overwrite => {
            let mut replaced = incoming.clone();
            reattach_credentials(&mut replaced, current);
            replaced
        }
        RestoreMode::Merge => {
            let mut merged = current.clone();
            merged.theme = incoming.theme;
            merged.language = incoming.language;
            merged.transcoding_quality = incoming.transcoding_quality;
            merged.audio_cache_limit_mb = incoming.audio_cache_limit_mb;
            merged.playback_modes = incoming.playback_modes;
            merged.auto_continue = incoming.auto_continue;
            merged.volume_normalization = incoming.volume_normalization;
            merged.pause_on_output_disconnect = incoming.pause_on_output_disconnect;
            merged.show_lyrics_translation = incoming.show_lyrics_translation;
            merged.show_track_notifications = incoming.show_track_notifications;
            merged.close_minimizes = incoming.close_minimizes;
            merged.shortcuts = incoming.shortcuts;
            merged.global_shortcuts = incoming.global_shortcuts;
            merged.lyrics = incoming.lyrics;
            merged.albums_sort = incoming.albums_sort;
            merged.artists_sort = incoming.artists_sort;
            merged.log_to_file = incoming.log_to_file;

            for server in incoming.servers {
                if !merged.servers.iter().any(|existing| existing.id == server.id) {
                    merged.servers.push(server);
                }
            }
            for source in incoming.sources {
                let id = source.id();
                if !merged.sources.iter().any(|existing| existing.id() == id) {
                    merged.sources.push(source);
                }
            }
            for proxy in incoming.network.proxies {
                if !merged
                    .network
                    .proxies
                    .iter()
                    .any(|existing| existing.id == proxy.id)
                {
                    merged.network.proxies.push(proxy);
                }
            }
            for route in incoming.network.source_proxies {
                if !merged
                    .network
                    .source_proxies
                    .iter()
                    .any(|existing| existing.source == route.source)
                {
                    merged.network.source_proxies.push(route);
                }
            }
            merged
        }
    };
    next.sync_sources();
    next
}

pub fn apply_equalizer_backup(
    current: &EqSettings,
    incoming: &EqSettings,
    mode: RestoreMode,
) -> EqSettings {
    match mode {
        RestoreMode::Overwrite => incoming.clone(),
        RestoreMode::Merge => {
            let mut merged = incoming.clone();
            for preset in &current.custom_presets {
                if !merged
                    .custom_presets
                    .iter()
                    .any(|p| p.name.eq_ignore_ascii_case(&preset.name))
                {
                    merged.custom_presets.push(preset.clone());
                }
            }
            merged
        }
    }
}

pub fn build_bundle(
    selection: BackupSelection,
    config: &Config,
    lyrics_cache: &crate::lyrics::cache::LyricsCache,
    local_playlists: &crate::local_playlists::LocalPlaylistStore,
    listening_log: &crate::listening_stats::ListeningLog,
) -> BackupBundle {
    let exported_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    BackupBundle {
        schema_version: BACKUP_SCHEMA_VERSION,
        exported_at,
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        config: selection.config.then(|| sanitize_config_for_backup(config)),
        equalizer: selection.equalizer.then(|| config.equalizer.clone()),
        lyrics_preferences: selection
            .lyrics_preferences
            .then(|| lyrics_cache.export_preferences()),
        local_playlists: selection
            .local_playlists
            .then(|| local_playlists.list().to_vec()),
        listening_log: selection
            .listening_log
            .then(|| listening_log.export_all()),
    }
}

pub fn write_bundle(path: &Path, bundle: &BackupBundle) -> Result<()> {
    crate::config::write_json_atomic(path, bundle, "backup bundle")
}

pub fn read_bundle(path: &Path) -> Result<BackupBundle> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read backup file {}", path.display()))?;
    let bundle: BackupBundle = serde_json::from_str(&raw)
        .with_context(|| format!("invalid backup file {}", path.display()))?;
    if bundle.schema_version == 0 || bundle.schema_version > BACKUP_SCHEMA_VERSION {
        bail!(
            "unsupported backup schema version {} (expected 1..={})",
            bundle.schema_version,
            BACKUP_SCHEMA_VERSION
        );
    }
    Ok(bundle)
}

pub fn write_safety_snapshot(
    backups_dir: &Path,
    config: &Config,
    lyrics_cache: &crate::lyrics::cache::LyricsCache,
    local_playlists: &crate::local_playlists::LocalPlaylistStore,
    listening_log: &crate::listening_stats::ListeningLog,
) -> Result<PathBuf> {
    fs::create_dir_all(backups_dir)
        .with_context(|| format!("failed to create {}", backups_dir.display()))?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let path = backups_dir.join(format!("pre-restore-{ts}.navibackup.json"));
    let bundle = build_bundle(
        BackupSelection::default(),
        config,
        lyrics_cache,
        local_playlists,
        listening_log,
    );
    write_bundle(&path, &bundle)?;
    Ok(path)
}

pub fn default_backups_dir() -> PathBuf {
    crate::config::config_dir().join("backups")
}

fn reattach_credentials(target: &mut Config, existing: &Config) {
    if !existing.password.is_empty()
        && existing.server_url == target.server_url
        && existing.username == target.username
    {
        target.password = existing.password.clone();
    }
    for server in &mut target.servers {
        if let Some(old) = existing.servers.iter().find(|s| s.id == server.id) {
            preserve_server_secret(server, old);
        }
    }
    for source in &mut target.sources {
        let id = source.id();
        let Some(old) = existing.sources.iter().find(|s| s.id() == id) else {
            continue;
        };
        match (source, old) {
            (
                SourceProfile::WebDav {
                    password,
                    password_in_keyring,
                    ..
                },
                SourceProfile::WebDav {
                    password: old_pw,
                    password_in_keyring: old_kr,
                    ..
                },
            ) => {
                *password = old_pw.clone();
                *password_in_keyring = *old_kr;
            }
            (
                SourceProfile::Bilibili {
                    account_id,
                    credential_key,
                    ..
                },
                SourceProfile::Bilibili {
                    account_id: old_acc,
                    credential_key: old_key,
                    ..
                },
            ) => {
                *account_id = *old_acc;
                *credential_key = old_key.clone();
            }
            _ => {}
        }
    }
}

fn preserve_server_secret(target: &mut ServerProfile, existing: &ServerProfile) {
    target.password = existing.password.clone();
    target.password_in_keyring = existing.password_in_keyring;
}

fn strip_url_userinfo(raw: &str) -> String {
    let Ok(mut parsed) = url::Url::parse(raw) else {
        return raw.to_string();
    };
    if !parsed.username().is_empty() || parsed.password().is_some() {
        let _ = parsed.set_username("");
        let _ = parsed.set_password(None);
        return parsed.to_string();
    }
    raw.to_string()
}
