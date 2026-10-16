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

/// Subsonic `getGenres` 返回的流派条目。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Genre {
    pub value: String,
    #[serde(default)]
    pub song_count: Option<i64>,
    #[serde(default)]
    pub album_count: Option<i64>,
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
    /// 碟号（Subsonic `discNumber`）；多碟专辑按（碟号， 曲目号）排序。
    #[serde(default)]
    pub disc: Option<i32>,
    #[serde(default)]
    pub duration: Option<i32>,
    #[serde(default)]
    pub cover_art: Option<String>,
    #[serde(default)]
    pub suffix: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub bit_rate: Option<i32>,
    #[serde(default)]
    pub size: Option<i64>,
    #[serde(default)]
    pub path: Option<String>,
    /// ReplayGain 曲目增益（dB）；Navidrome/Subsonic 可选字段，缺失时安全回退。
    #[serde(
        default,
        alias = "replayGainTrack",
        deserialize_with = "deserialize_optional_f32"
    )]
    pub replay_gain_track_gain: Option<f32>,
    #[serde(default, deserialize_with = "deserialize_optional_f32")]
    pub replay_gain_track_peak: Option<f32>,
    #[serde(
        default,
        alias = "replayGainAlbum",
        deserialize_with = "deserialize_optional_f32"
    )]
    pub replay_gain_album_gain: Option<f32>,
    #[serde(default, deserialize_with = "deserialize_optional_f32")]
    pub replay_gain_album_peak: Option<f32>,
}

/// 容错解析可选的 f32 字段：接受数字或字符串，其他情况返回 None，
/// 避免个别字段类型不符导致整首歌解析失败。
fn deserialize_optional_f32<'de, D>(deserializer: D) -> Result<Option<f32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| match value {
        serde_json::Value::Number(number) => number.as_f64().map(|n| n as f32),
        serde_json::Value::String(text) => text.parse::<f32>().ok(),
        _ => None,
    }))
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
    /// 其他语言变体中按时间戳就近匹配到的翻译行；无可用变体时为 None。
    pub translation: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ServerInfo {
    pub version: String,
    pub app_name: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackMode {
    #[default]
    Sequential,
    RepeatAll,
    RepeatOne,
    Shuffle,
}

/// 循环模式：不循环 / 列表循环 / 单曲循环。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            Self::Off => "Repeat off",
            Self::All => "Repeat all",
            Self::One => "Repeat current track",
        }
    }
}

/// 播放模式：随机开关与循环模式相互独立（与 MPRIS 及常见播放器的
/// 二维模型一致）。旧版单枚举 [`PlaybackMode`] 仅用于配置/会话迁移。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PlayModes {
    #[serde(default)]
    pub shuffle: bool,
    #[serde(default)]
    pub repeat: RepeatMode,
}

impl PlayModes {
    /// 旧版四态枚举到二维模式的迁移映射：随机枚举只保留随机开关，
    /// 循环枚举只保留循环模式。
    pub fn from_legacy(mode: PlaybackMode) -> Self {
        match mode {
            PlaybackMode::Sequential => Self {
                shuffle: false,
                repeat: RepeatMode::Off,
            },
            PlaybackMode::RepeatAll => Self {
                shuffle: false,
                repeat: RepeatMode::All,
            },
            PlaybackMode::RepeatOne => Self {
                shuffle: false,
                repeat: RepeatMode::One,
            },
            PlaybackMode::Shuffle => Self {
                shuffle: true,
                repeat: RepeatMode::Off,
            },
        }
    }
}

/// 专辑列表的排序方式。除 `NewestAdded` 外均直接映射到
/// Subsonic `getAlbumList2` 的服务端排序，保持增量分页可用。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlbumSort {
    /// 最近添加（`type=newest`）。
    #[default]
    NewestAdded,
    /// 名称 A→Z（`type=alphabeticalByName`）。
    Name,
    /// 艺术家名称 A→Z（`type=alphabeticalByArtist`）。
    Artist,
    /// 发行年份新→旧（`type=byYear`，fromYear > toYear 表示倒序）。
    Year,
}

impl AlbumSort {
    pub fn label(self) -> &'static str {
        match self {
            Self::NewestAdded => "Recently added",
            Self::Name => "Name",
            Self::Artist => "Artist",
            Self::Year => "Year",
        }
    }

    pub const ALL: [Self; 4] = [Self::NewestAdded, Self::Name, Self::Artist, Self::Year];
}

/// 艺术家列表的排序方式。`getArtists` 不支持服务端排序参数，
/// 全量列表已一次性加载，在客户端排序。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtistSort {
    /// 名称 A→Z（服务器索引顺序）。
    #[default]
    Name,
    /// 名称 Z→A。
    NameDesc,
    /// 专辑数多→少，同名按 A→Z。
    AlbumCount,
}

impl ArtistSort {
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name (A–Z)",
            Self::NameDesc => "Name (Z–A)",
            Self::AlbumCount => "Album count",
        }
    }

    pub const ALL: [Self; 3] = [Self::Name, Self::NameDesc, Self::AlbumCount];
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PlaybackSession {
    /// 归属服务器（切换服务器时不恢复其他服务器的会话）。
    pub server_url: String,
    /// 播放队列快照。
    pub queue: Vec<Song>,
    /// 当前播放的队列索引。
    pub queue_index: Option<usize>,
    /// 播放位置（秒）。
    pub position_secs: f64,
    /// 播放模式（随机开关 + 循环模式）。
    pub playback_modes: PlayModes,
    /// 随机播放历史（播放过的索引）。
    pub shuffle_played: Vec<usize>,
    /// 随机播放回退暂存。
    pub shuffle_forward: Vec<usize>,
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
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// 播放模式（随机开关 + 循环模式）。
    #[serde(default)]
    pub playback_modes: PlayModes,
    #[serde(default)]
    pub volume_normalization: VolumeNormalization,
    /// 侧边栏是否折叠为图标栏。
    #[serde(default)]
    pub sidebar_collapsed: bool,
    /// 歌词是否显示翻译行（服务器返回多语言变体时）。
    #[serde(default = "default_true")]
    pub show_lyrics_translation: bool,
    /// 音频输出设备名；None 表示跟随系统默认设备。
    #[serde(default)]
    pub output_device: Option<String>,
    /// 专辑列表排序方式。
    #[serde(default)]
    pub albums_sort: AlbumSort,
    /// 艺术家列表排序方式。
    #[serde(default)]
    pub artists_sort: ArtistSort,
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
            volume: default_volume(),
            playback_modes: PlayModes::default(),
            volume_normalization: VolumeNormalization::Track,
            sidebar_collapsed: false,
            show_lyrics_translation: default_true(),
            output_device: None,
            albums_sort: AlbumSort::default(),
            artists_sort: ArtistSort::default(),
        }
    }
}

fn default_volume() -> f32 {
    0.7
}

fn default_true() -> bool {
    true
}

/// 音量标准化模式：关闭 / 按曲目 / 按专辑（ReplayGain）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VolumeNormalization {
    Off,
    #[default]
    Track,
    Album,
}

impl VolumeNormalization {
    pub const ALL: [Self; 3] = [Self::Off, Self::Track, Self::Album];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Track => "Track",
            Self::Album => "Album",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            Self::Off => "Play every song at its recorded loudness",
            Self::Track => "Normalize each track to a consistent loudness (ReplayGain)",
            Self::Album => "Normalize by album so a record's relative dynamics are preserved",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        AlbumSort, ArtistSort, Config, PlayModes, PlaybackMode, PlaybackSession, RepeatMode, Song,
        ThemePreference, TranscodingQuality, VolumeNormalization,
    };

    #[test]
    fn legacy_config_defaults_to_light_theme() {
        let config: Config = serde_json::from_str(
            r#"{"server_url":"http://localhost:4533","username":"user","password":"pass"}"#,
        )
        .expect("legacy config should still deserialize");

        assert_eq!(config.theme, ThemePreference::Light);
        assert_eq!(config.cache_dir, None);
        assert_eq!(config.transcoding_quality, TranscodingQuality::Original);
        assert_eq!(config.volume_normalization, VolumeNormalization::Track);
        assert!((config.volume - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn song_parses_replay_gain_fields_tolerantly() {
        let song: Song = serde_json::from_str(
            r#"{
                "id": "s1", "title": "Track",
                "replayGainTrackGain": -3.5,
                "replayGainTrackPeak": "0.98",
                "replayGainAlbumGain": -2.1,
                "replayGainAlbumPeak": 1.0,
                "replayGainWeird": "not-a-number"
            }"#,
        )
        .expect("song should parse despite mixed replay gain types");

        assert_eq!(song.replay_gain_track_gain, Some(-3.5));
        assert_eq!(song.replay_gain_track_peak, Some(0.98));
        assert_eq!(song.replay_gain_album_gain, Some(-2.1));
        assert_eq!(song.replay_gain_album_peak, Some(1.0));
    }

    #[test]
    fn volume_normalization_round_trips_through_config_json() {
        let config = Config {
            volume_normalization: VolumeNormalization::Album,
            ..Config::default()
        };
        let text = serde_json::to_string(&config).expect("config should serialize");
        let parsed: Config = serde_json::from_str(&text).expect("config should round-trip");
        assert_eq!(parsed.volume_normalization, VolumeNormalization::Album);
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
            volume: 0.42,
            ..Config::default()
        };

        let json = serde_json::to_string(&config).unwrap();
        let restored: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.cache_dir, config.cache_dir);
        assert_eq!(restored.transcoding_quality, TranscodingQuality::Kbps192);
        assert!((restored.volume - 0.42).abs() < f32::EPSILON);
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

    #[test]
    fn legacy_playback_mode_maps_to_independent_shuffle_and_repeat() {
        for (legacy, shuffle, repeat) in [
            (PlaybackMode::Sequential, false, RepeatMode::Off),
            (PlaybackMode::RepeatAll, false, RepeatMode::All),
            (PlaybackMode::RepeatOne, false, RepeatMode::One),
            (PlaybackMode::Shuffle, true, RepeatMode::Off),
        ] {
            let modes = PlayModes::from_legacy(legacy);
            assert_eq!(modes.shuffle, shuffle);
            assert_eq!(modes.repeat, repeat);
        }
        assert_eq!(
            PlayModes::default(),
            PlayModes {
                shuffle: false,
                repeat: RepeatMode::Off
            }
        );
    }

    #[test]
    fn repeat_mode_cycles_off_all_one() {
        assert_eq!(RepeatMode::Off.next(), RepeatMode::All);
        assert_eq!(RepeatMode::All.next(), RepeatMode::One);
        assert_eq!(RepeatMode::One.next(), RepeatMode::Off);
    }

    #[test]
    fn legacy_config_defaults_to_sequential_playback_modes() {
        let config: Config = serde_json::from_str(
            r#"{"server_url":"http://localhost:4533","username":"user","password":"pass"}"#,
        )
        .expect("legacy config without playback modes should still deserialize");

        assert_eq!(
            config.playback_modes,
            PlayModes {
                shuffle: false,
                repeat: RepeatMode::Off
            }
        );
    }

    #[test]
    fn legacy_config_defaults_to_default_library_sorts() {
        let config: Config = serde_json::from_str(
            r#"{"server_url":"http://localhost:4533","username":"user","password":"pass"}"#,
        )
        .expect("legacy config without sort fields should still deserialize");

        assert_eq!(config.albums_sort, AlbumSort::NewestAdded);
        assert_eq!(config.artists_sort, ArtistSort::Name);
    }

    #[test]
    fn output_device_defaults_to_following_system_default() {
        let config: Config = serde_json::from_str(
            r#"{"server_url":"http://localhost:4533","username":"user","password":"pass"}"#,
        )
        .expect("legacy config without output device should still deserialize");
        assert_eq!(config.output_device, None);

        let json = serde_json::to_string(&Config {
            output_device: Some("Speakers".to_string()),
            ..Config::default()
        })
        .expect("config should serialize");
        let restored: Config = serde_json::from_str(&json).expect("config should deserialize");
        assert_eq!(restored.output_device.as_deref(), Some("Speakers"));
    }

    #[test]
    fn legacy_config_defaults_to_showing_lyrics_translation() {
        let config: Config = serde_json::from_str(
            r#"{"server_url":"http://localhost:4533","username":"user","password":"pass"}"#,
        )
        .expect("legacy config without lyrics settings should still deserialize");
        assert!(config.show_lyrics_translation);

        let json = serde_json::to_string(&Config {
            show_lyrics_translation: false,
            ..Config::default()
        })
        .expect("config should serialize");
        let restored: Config = serde_json::from_str(&json).expect("config should deserialize");
        assert!(!restored.show_lyrics_translation);
        assert!(json.contains("show_lyrics_translation"));
    }

    #[test]
    fn library_sorts_round_trip_through_config_json() {
        let config = Config {
            albums_sort: AlbumSort::Year,
            artists_sort: ArtistSort::AlbumCount,
            ..Config::default()
        };

        let json = serde_json::to_string(&config).expect("config should serialize");
        let restored: Config = serde_json::from_str(&json).expect("config should deserialize");

        assert_eq!(restored.albums_sort, AlbumSort::Year);
        assert_eq!(restored.artists_sort, ArtistSort::AlbumCount);
        assert!(json.contains(r#""albums_sort":"year""#));
        assert!(json.contains(r#""artists_sort":"album_count""#));
    }

    #[test]
    fn playback_modes_round_trip_through_config_json() {
        let config = Config {
            playback_modes: PlayModes {
                shuffle: true,
                repeat: RepeatMode::One,
            },
            ..Config::default()
        };

        let json = serde_json::to_string(&config).expect("config should serialize");
        let restored: Config = serde_json::from_str(&json).expect("config should deserialize");

        assert!(restored.playback_modes.shuffle);
        assert_eq!(restored.playback_modes.repeat, RepeatMode::One);
        assert!(json.contains(r#""playback_modes":{"shuffle":true,"repeat":"one"}"#));
    }

    #[test]
    fn playback_session_round_trips_through_json() {
        let session = PlaybackSession {
            server_url: "http://localhost:4533".to_string(),
            queue: vec![Song::default()],
            queue_index: Some(2),
            position_secs: 61.5,
            playback_modes: PlayModes {
                shuffle: true,
                repeat: RepeatMode::All,
            },
            shuffle_played: vec![0, 3, 1],
            shuffle_forward: vec![2],
        };

        let json = serde_json::to_string(&session).expect("session should serialize");
        let restored: PlaybackSession =
            serde_json::from_str(&json).expect("session should deserialize");

        assert_eq!(restored.server_url, session.server_url);
        assert_eq!(restored.queue.len(), 1);
        assert_eq!(restored.queue_index, Some(2));
        assert!((restored.position_secs - 61.5).abs() < 1e-9);
        assert!(restored.playback_modes.shuffle);
        assert_eq!(restored.playback_modes.repeat, RepeatMode::All);
        assert_eq!(restored.shuffle_played, vec![0, 3, 1]);
        assert_eq!(restored.shuffle_forward, vec![2]);
    }

    #[test]
    fn corrupted_session_json_fails_cleanly() {
        assert!(serde_json::from_str::<PlaybackSession>("not json at all").is_err());
    }
}
