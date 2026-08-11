use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FavoriteKind {
    Artist,
    Album,
    Song,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FavoriteKey {
    pub kind: FavoriteKind,
    pub id: String,
}

impl FavoriteKey {
    pub fn new(kind: FavoriteKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    Light,
    Dark,
    System,
}

impl ThemePreference {
    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::System => "System",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscodingQuality {
    #[default]
    Original,
    Kbps128,
    Kbps192,
    Kbps256,
    Kbps320,
}

impl TranscodingQuality {
    pub const ALL: [Self; 5] = [
        Self::Original,
        Self::Kbps128,
        Self::Kbps192,
        Self::Kbps256,
        Self::Kbps320,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Original => "Original",
            Self::Kbps128 => "128 kbps",
            Self::Kbps192 => "192 kbps",
            Self::Kbps256 => "256 kbps",
            Self::Kbps320 => "320 kbps",
        }
    }

    pub fn max_bit_rate(self) -> Option<u32> {
        match self {
            Self::Original => None,
            Self::Kbps128 => Some(128),
            Self::Kbps192 => Some(192),
            Self::Kbps256 => Some(256),
            Self::Kbps320 => Some(320),
        }
    }

    pub fn cache_profile(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::Kbps128 => "mp3-128",
            Self::Kbps192 => "mp3-192",
            Self::Kbps256 => "mp3-256",
            Self::Kbps320 => "mp3-320",
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artist {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub album_count: Option<i32>,
    #[serde(default)]
    pub cover_art: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub artist_id: Option<String>,
    #[serde(default)]
    pub cover_art: Option<String>,
    #[serde(default)]
    pub song_count: Option<i32>,
    #[serde(default)]
    pub duration: Option<i32>,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub genre: Option<String>,
    #[serde(default)]
    pub created: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Song {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub album_id: Option<String>,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub artist_id: Option<String>,
    #[serde(default)]
    pub track: Option<i32>,
    #[serde(default)]
    pub duration: Option<i32>,
    #[serde(default)]
    pub cover_art: Option<String>,
    #[serde(default)]
    pub suffix: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub size: Option<i64>,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub song_count: Option<i32>,
    #[serde(default)]
    pub duration: Option<i32>,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub public: Option<bool>,
    #[serde(default)]
    pub cover_art: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct SearchResults {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub songs: Vec<Song>,
}

#[derive(Clone, Debug, Default)]
pub struct Favorites {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub songs: Vec<Song>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lyrics {
    pub display_artist: Option<String>,
    pub display_title: Option<String>,
    pub lines: Vec<LyricLine>,
}

impl Lyrics {
    pub fn is_synced(&self) -> bool {
        self.lines.iter().any(|line| line.start_ms.is_some())
    }

    pub fn active_line_index(&self, position: std::time::Duration) -> Option<usize> {
        let position_ms = u64::try_from(position.as_millis()).unwrap_or(u64::MAX);
        self.lines
            .iter()
            .enumerate()
            .rev()
            .find(|(_, line)| line.start_ms.is_some_and(|start| start <= position_ms))
            .map(|(index, _)| index)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LyricLine {
    pub start_ms: Option<u64>,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct ServerInfo {
    pub version: String,
    pub app_name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub server_url: String,
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub theme: ThemePreference,
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
    #[serde(default)]
    pub transcoding_quality: TranscodingQuality,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server_url: "http://127.0.0.1:4533".to_string(),
            username: String::new(),
            password: String::new(),
            theme: ThemePreference::Light,
            cache_dir: None,
            transcoding_quality: TranscodingQuality::Original,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{Config, ThemePreference, TranscodingQuality};

    #[test]
    fn legacy_config_defaults_to_light_theme() {
        let config: Config = serde_json::from_str(
            r#"{"server_url":"http://localhost:4533","username":"user","password":"pass"}"#,
        )
        .expect("legacy config should still deserialize");

        assert_eq!(config.theme, ThemePreference::Light);
        assert_eq!(config.cache_dir, None);
        assert_eq!(config.transcoding_quality, TranscodingQuality::Original);
    }

    #[test]
    fn transcoding_quality_uses_supported_subsonic_rates() {
        assert_eq!(TranscodingQuality::Original.max_bit_rate(), None);
        assert_eq!(TranscodingQuality::Kbps192.max_bit_rate(), Some(192));
        assert_eq!(TranscodingQuality::Kbps320.max_bit_rate(), Some(320));
    }

    #[test]
    fn playback_settings_round_trip_through_json() {
        let config = Config {
            cache_dir: Some(PathBuf::from("D:/Music Cache")),
            transcoding_quality: TranscodingQuality::Kbps192,
            ..Config::default()
        };

        let json = serde_json::to_string(&config).unwrap();
        let restored: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.cache_dir, config.cache_dir);
        assert_eq!(restored.transcoding_quality, TranscodingQuality::Kbps192);
    }

    #[test]
    fn theme_preference_uses_stable_config_names() {
        let config = Config {
            theme: ThemePreference::System,
            ..Config::default()
        };

        let json = serde_json::to_string(&config).expect("config should serialize");

        assert!(json.contains(r#""theme":"system""#));
    }
}
