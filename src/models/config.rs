use std::path::PathBuf;

use md5::{Digest, Md5};
use rust_i18n::t;
use serde::{Deserialize, Serialize};

use super::source::{ProviderKind, SourceId, SourceProfile};
use super::{
    AlbumSort, ArtistSort, EqSettings, GlobalShortcutSettings, LanguagePreference, PlayModes,
    ShortcutSettings, ThemePreference, TranscodingQuality, VolumeNormalization,
};

mod sources;

/// 一台配置好的服务端。`id` 由地址与用户名派生，用作缓存命名空间。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerProfile {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub url: String,
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub password_in_keyring: bool,
}

impl ServerProfile {
    pub fn id_for(url: &str, username: &str) -> String {
        let mut hasher = Md5::new();
        hasher.update(normalize_server_url(url).as_bytes());
        hasher.update(b"\n");
        hasher.update(username.trim().as_bytes());
        format!("{:x}", hasher.finalize())[..12].to_string()
    }

    pub fn new(url: &str, username: &str, password: &str) -> Self {
        Self {
            id: Self::id_for(url, username),
            name: String::new(),
            url: url.trim().to_string(),
            username: username.trim().to_string(),
            password: password.to_string(),
            password_in_keyring: false,
        }
    }

    pub fn resolved_password(&self) -> String {
        if self.password_in_keyring {
            if let Some((_, password)) = crate::secrets::load_password(&self.id) {
                return password;
            }
        }
        self.password.clone()
    }

    pub fn label(&self) -> String {
        if !self.name.trim().is_empty() {
            return self.name.trim().to_string();
        }
        let host = self.host();
        if host.is_empty() {
            t!("Unnamed server").to_string()
        } else {
            host
        }
    }

    pub fn host(&self) -> String {
        normalize_server_url(&self.url)
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(self.url.trim())
            .split('/')
            .next()
            .unwrap_or_default()
            .to_string()
    }

    pub fn is_configured(&self) -> bool {
        !self.url.trim().is_empty() && !self.username.trim().is_empty()
    }
}

fn normalize_server_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };
    with_scheme.trim_end_matches('/').to_lowercase()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    /// 当前激活服务器的地址/账号镜像，保留用于旧配置兼容。
    #[serde(default)]
    pub server_url: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub servers: Vec<ServerProfile>,
    #[serde(default)]
    pub active_server: Option<String>,
    /// Source profiles share existing OpenSubsonic credential references.
    #[serde(default, deserialize_with = "sources::deserialize_profiles")]
    pub sources: Vec<SourceProfile>,
    /// Browsing source; playback will keep its own source context.
    #[serde(default, deserialize_with = "sources::deserialize_active")]
    pub active_source: Option<SourceId>,
    /// Last accepted playback owner, independent of the selected browsing source.
    #[serde(default, deserialize_with = "sources::deserialize_active")]
    pub playback_source: Option<SourceId>,
    #[serde(default)]
    pub theme: ThemePreference,
    #[serde(default)]
    pub language: LanguagePreference,
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
    #[serde(default)]
    pub transcoding_quality: TranscodingQuality,
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default)]
    pub playback_modes: PlayModes,
    #[serde(default)]
    pub auto_continue: bool,
    #[serde(default)]
    pub volume_normalization: VolumeNormalization,
    #[serde(default)]
    pub sidebar_collapsed: bool,
    #[serde(default = "default_true")]
    pub show_lyrics_translation: bool,
    #[serde(default)]
    pub output_device: Option<String>,
    #[serde(default)]
    pub equalizer: EqSettings,
    #[serde(default)]
    pub albums_sort: AlbumSort,
    #[serde(default)]
    pub artists_sort: ArtistSort,
    #[serde(default = "default_true")]
    pub show_track_notifications: bool,
    #[serde(default)]
    pub close_minimizes: bool,
    #[serde(default)]
    pub shortcuts: ShortcutSettings,
    #[serde(default)]
    pub global_shortcuts: GlobalShortcutSettings,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server_url: "http://127.0.0.1:4533".to_string(),
            username: String::new(),
            password: String::new(),
            servers: Vec::new(),
            active_server: None,
            sources: Vec::new(),
            active_source: None,
            playback_source: None,
            theme: ThemePreference::Light,
            language: LanguagePreference::System,
            cache_dir: None,
            transcoding_quality: TranscodingQuality::Original,
            volume: default_volume(),
            playback_modes: PlayModes::default(),
            auto_continue: false,
            volume_normalization: VolumeNormalization::Track,
            sidebar_collapsed: false,
            show_lyrics_translation: default_true(),
            output_device: None,
            equalizer: EqSettings::default(),
            albums_sort: AlbumSort::default(),
            artists_sort: ArtistSort::default(),
            show_track_notifications: default_true(),
            close_minimizes: false,
            shortcuts: ShortcutSettings::default(),
            global_shortcuts: GlobalShortcutSettings::default(),
        }
    }
}

impl Config {
    pub fn sync_active_profile(&mut self) {
        let has_bilibili = self.sources.iter().any(
            |source| matches!(source, SourceProfile::Bilibili { id, .. } if !id.trim().is_empty()),
        );
        let configured_legacy =
            !self.server_url.trim().is_empty() && !self.username.trim().is_empty();
        if self.servers.is_empty() && (!has_bilibili || configured_legacy) {
            let profile = ServerProfile::new(&self.server_url, &self.username, &self.password);
            self.active_server = Some(profile.id.clone());
            self.servers.push(profile);
        }
        let active_exists = self
            .active_server
            .as_ref()
            .is_some_and(|id| self.servers.iter().any(|profile| &profile.id == id));
        if !active_exists {
            self.active_server = self.servers.first().map(|profile| profile.id.clone());
        }
        self.sync_sources();
        if self
            .active_source
            .as_ref()
            .is_some_and(|source| source.provider == ProviderKind::OpenSubsonic)
        {
            if let Some(profile) = self.active_profile().cloned() {
                self.server_url = profile.url;
                self.username = profile.username;
                self.password = profile.password;
            }
        } else if self
            .active_profile()
            .is_some_and(|profile| profile.password_in_keyring)
        {
            // Credential migration must remove the last server's plaintext
            // mirror even when the user is browsing another provider.
            self.password.clear();
        }
    }

    pub fn active_profile(&self) -> Option<&ServerProfile> {
        let id = self.active_server.as_ref()?;
        self.servers.iter().find(|profile| &profile.id == id)
    }

    pub fn active_server_id(&self) -> Option<&str> {
        self.active_profile().map(|profile| profile.id.as_str())
    }

    pub fn switch_to(&mut self, id: &str) -> bool {
        if !self.servers.iter().any(|server| server.id == id) {
            return false;
        }
        self.sync_sources();
        self.switch_source(&SourceId::new(ProviderKind::OpenSubsonic, id))
    }

    fn activate_server(&mut self, id: &str) -> bool {
        let Some(profile) = self
            .servers
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
        else {
            return false;
        };
        self.active_source = Some(SourceId::new(ProviderKind::OpenSubsonic, &profile.id));
        self.active_server = Some(profile.id);
        self.server_url = profile.url;
        self.username = profile.username;
        self.password = profile.password;
        self.sync_sources();
        true
    }

    pub fn store_active(&mut self, url: &str, username: &str, password: &str) -> String {
        let url = url.trim().to_string();
        let username = username.trim().to_string();
        let password = password.to_string();
        let derived = ServerProfile::id_for(&url, &username);
        let duplicate = self
            .servers
            .iter()
            .find(|profile| profile.id == derived)
            .map(|profile| profile.id.clone());

        if let Some(id) = duplicate.filter(|id| Some(id.as_str()) != self.active_server.as_deref())
        {
            if let Some(profile) = self.servers.iter_mut().find(|profile| profile.id == id) {
                profile.url = url;
                profile.username = username;
                profile.password = password;
                profile.password_in_keyring = false;
            }
            self.switch_to(&id);
            return id;
        }

        match self.active_server.clone() {
            Some(id) if self.servers.iter().any(|profile| profile.id == id) => {
                if let Some(profile) = self.servers.iter_mut().find(|profile| profile.id == id) {
                    profile.url = url;
                    profile.username = username;
                    profile.password = password;
                }
            }
            _ => {
                let profile = ServerProfile {
                    id: derived,
                    name: String::new(),
                    url,
                    username,
                    password,
                    password_in_keyring: false,
                };
                let id = profile.id.clone();
                self.servers.push(profile);
                self.active_server = Some(id);
            }
        }
        self.sync_active_profile();
        self.active_server.clone().unwrap_or_default()
    }

    pub fn store_profile(&mut self, id: &str, url: &str, username: &str, password: &str) -> bool {
        let Some(profile) = self.servers.iter_mut().find(|profile| profile.id == id) else {
            return false;
        };
        profile.url = url.trim().to_string();
        profile.username = username.trim().to_string();
        profile.password = password.to_string();
        profile.password_in_keyring = false;
        true
    }

    pub fn add_server(&mut self) -> String {
        let base = ServerProfile::id_for(&self.server_url, &self.username);
        let mut candidate = base.clone();
        let mut serial = 2;
        while self.servers.iter().any(|profile| profile.id == candidate) {
            candidate = format!("{base}-{serial}");
            serial += 1;
        }
        let profile = ServerProfile {
            id: candidate.clone(),
            name: t!("Server %{number}", number = self.servers.len() + 1).to_string(),
            url: String::new(),
            username: String::new(),
            password: String::new(),
            password_in_keyring: false,
        };
        self.servers.push(profile);
        self.switch_to(&candidate);
        candidate
    }

    pub fn remove_server(&mut self, id: &str) -> bool {
        let has_bilibili = self.sources.iter().any(
            |source| matches!(source, SourceProfile::Bilibili { id, .. } if !id.trim().is_empty()),
        );
        if self.servers.len() <= 1 && !has_bilibili {
            return false;
        }
        let Some(index) = self.servers.iter().position(|profile| profile.id == id) else {
            return false;
        };
        let was_active = self.active_server.as_deref() == Some(id);
        let browsing_bilibili = self
            .active_source
            .as_ref()
            .filter(|source| source.provider == ProviderKind::Bilibili)
            .cloned();
        self.servers.remove(index);
        if was_active {
            if let Some(next) = self
                .servers
                .get(index.min(self.servers.len().saturating_sub(1)))
            {
                let next = next.id.clone();
                self.switch_to(&next);
            } else {
                self.active_server = None;
                self.server_url.clear();
                self.username.clear();
                self.password.clear();
            }
        }
        if let Some(source) = browsing_bilibili {
            self.active_source = Some(source);
        }
        self.sync_active_profile();
        true
    }
}

fn default_volume() -> f32 {
    0.7
}

fn default_true() -> bool {
    true
}
