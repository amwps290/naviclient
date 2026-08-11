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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server_url: "http://127.0.0.1:4533".to_string(),
            username: String::new(),
            password: String::new(),
            theme: ThemePreference::Light,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, ThemePreference};

    #[test]
    fn legacy_config_defaults_to_light_theme() {
        let config: Config = serde_json::from_str(
            r#"{"server_url":"http://localhost:4533","username":"user","password":"pass"}"#,
        )
        .expect("legacy config should still deserialize");

        assert_eq!(config.theme, ThemePreference::Light);
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
