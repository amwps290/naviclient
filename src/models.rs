use std::path::PathBuf;

use md5::{Digest, Md5};
use rust_i18n::t;
use serde::{Deserialize, Serialize};

mod listening;
mod media;
mod playback;
mod preferences;
mod queue;
mod server;
mod shortcuts;
mod sort;
#[cfg(test)]
use listening::LISTENING_HISTORY_LIMIT;
pub use listening::{ListeningEvent, ListeningHistory};
pub use media::{
    Album, Artist, Favorites, Genre, LyricLine, Lyrics, Playlist, SearchResults, Song,
};
pub use playback::{
    EqSettings, PlayModes, PlaybackMode, RepeatMode, TranscodingQuality, VolumeNormalization,
};
#[cfg(test)]
use preferences::locale_from_os_tag;
pub use preferences::{LanguagePreference, ThemePreference};
pub use queue::{normalize_queue_origins, PlaybackSession, QueueOrigin};
pub use server::{CapabilitySupport, OpenSubsonicExtension, ServerCapabilities, ServerInfo};
pub use shortcuts::{GlobalShortcutSettings, ShortcutAction, ShortcutSettings};
pub use sort::{AlbumSort, ArtistSort};

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

/// 一台配置好的服务端。`id` 由地址与用户名派生，用作缓存目录命名空间
/// （播放会话、响度、流派缓存都按服务器隔离，song id 在不同服务器之间会撞）。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerProfile {
    pub id: String,
    /// 用户自定义的显示名；为空时回退到主机名。
    #[serde(default)]
    pub name: String,
    pub url: String,
    pub username: String,
    pub password: String,
    /// 密码已迁入系统凭据库：配置里的 `password` 保持为空，连接时从
    /// 凭据库读取。读取失败（凭据库被锁/被清）时回退明文字段——迁移
    /// 中断、跨机器拷贝配置等场景仍可连接。
    #[serde(default)]
    pub password_in_keyring: bool,
}

impl ServerProfile {
    /// 由地址 + 用户名派生稳定 id：同一服务器反复添加不会产生重复条目，
    /// 重启后缓存命名空间也不会变。
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

    /// 连接时实际使用的密码：迁入系统凭据库的条目优先读凭据库，
    /// 凭据库不可用或读取失败时回退明文字段。
    pub fn resolved_password(&self) -> String {
        if self.password_in_keyring {
            if let Some((_, password)) = crate::secrets::load_password(&self.id) {
                return password;
            }
        }
        self.password.clone()
    }

    /// 界面上的短名：优先用户命名，否则取主机名（去掉协议与端口）。
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

    /// 仅主机名，用于紧凑展示。
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

/// 比较与派生 id 时使用的规范形式：去空白、去尾部斜杠、缺协议补 http。
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
    /// 当前激活服务器的地址/账号镜像。
    ///
    /// 保留为独立字段是为了兼容旧配置与 `OpenSubsonicProvider::new` 等现有调用点；
    /// `sync_active_profile` 会保证它始终与 `servers[active_server]` 一致。
    pub server_url: String,
    pub username: String,
    pub password: String,
    /// 已保存的所有服务端；始终至少一条（迁移后）。
    #[serde(default)]
    pub servers: Vec<ServerProfile>,
    /// 当前激活服务器的 id。
    #[serde(default)]
    pub active_server: Option<String>,
    #[serde(default)]
    pub theme: ThemePreference,
    /// 界面语言（System = 跟随操作系统）。
    #[serde(default)]
    pub language: LanguagePreference,
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
    #[serde(default)]
    pub transcoding_quality: TranscodingQuality,
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// 播放模式（随机开关 + 循环模式）。
    #[serde(default)]
    pub playback_modes: PlayModes,
    /// 顺序播放自然到达队尾后，是否生成推荐曲目继续播放。
    #[serde(default)]
    pub auto_continue: bool,
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
    /// 均衡器设置。
    #[serde(default)]
    pub equalizer: EqSettings,
    /// 专辑列表排序方式。
    #[serde(default)]
    pub albums_sort: AlbumSort,
    /// 艺术家列表排序方式。
    #[serde(default)]
    pub artists_sort: ArtistSort,
    /// 切歌时是否发送系统通知（标题/艺人/专辑/封面）。
    #[serde(default = "default_true")]
    pub show_track_notifications: bool,
    /// 窗口关闭按钮是否改为最小化（而非退出）。Linux Wayland 下部分
    /// 合成器会忽略最小化请求，故默认关闭。
    #[serde(default)]
    pub close_minimizes: bool,
    /// 可自定义的应用内快捷键。
    #[serde(default)]
    pub shortcuts: ShortcutSettings,
    /// 应用失焦或隐藏后仍可使用的系统全局播放快捷键。
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
    /// 保证服务器列表不变量成立。
    ///
    /// 三条不变量：至少一条（旧版单服务器配置迁移成一条）、`active_server`
    /// 指向存在的条目、遗留的 `server_url/username/password` 与激活条目一致。
    /// 启动与每次保存后调用，历史配置无需手工迁移。
    pub fn sync_active_profile(&mut self) {
        if self.servers.is_empty() {
            // 空列表：把遗留字段当成唯一服务器收编（全新安装也会得到一条可编辑条目）。
            let profile = ServerProfile::new(&self.server_url, &self.username, &self.password);
            self.active_server = Some(profile.id.clone());
            self.servers.push(profile);
        }
        let active_exists = self
            .active_server
            .as_ref()
            .is_some_and(|id| self.servers.iter().any(|profile| &profile.id == id));
        if !active_exists {
            self.active_server = Some(self.servers[0].id.clone());
        }
        if let Some(profile) = self.active_profile().cloned() {
            self.server_url = profile.url;
            self.username = profile.username;
            self.password = profile.password;
        }
    }

    pub fn active_profile(&self) -> Option<&ServerProfile> {
        let id = self.active_server.as_ref()?;
        self.servers.iter().find(|profile| &profile.id == id)
    }

    /// 当前服务器的缓存命名空间键；未配置时返回 None。
    pub fn active_server_id(&self) -> Option<&str> {
        self.active_profile().map(|profile| profile.id.as_str())
    }

    /// 切到指定服务器。失败（id 不存在）时保持原状并返回 false。
    pub fn switch_to(&mut self, id: &str) -> bool {
        let Some(profile) = self
            .servers
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
        else {
            return false;
        };
        self.active_server = Some(profile.id);
        self.server_url = profile.url;
        self.username = profile.username;
        self.password = profile.password;
        true
    }

    /// 把当前输入框里的地址/账号写回激活条目。
    ///
    /// 条目 id 保持不变（即使地址变了），这样缓存命名空间不会因为改了一个
    /// 端口就全部丢失；若新地址 + 用户名已属于另一条，则合并到那一条。
    /// 返回激活的服务器 id。
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

        // 新身份落在别的条目上：直接激活那一条，不动原条目。
        // 写入新的明文密码时必须落下凭据库标记，否则连接时会优先读凭据库里
        // 那条旧密码；后续 save_settings 会把新密码重新写入凭据库。
        if let Some(id) = duplicate.filter(|id| Some(id.as_str()) != self.active_server.as_deref())
        {
            if let Some(profile) = self.servers.iter_mut().find(|p| p.id == id) {
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

    /// 把行内编辑结果写回一个非激活条目（设置页每行的铅笔按钮）。
    ///
    /// 与 store_active 不同：不微合并、不激活——修另一台服务器的地址
    /// 账号不应把正在连接的服务器切走。条目 id 保持不变，缓存命名空间
    /// 不因改了地址/账号而丢失。密码先回退为明文、凭据库标记清零，
    /// 由调用方后续的凭据库写入逻辑统一处理（与激活路径同一套）。
    /// 找不到条目返回 false。
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

    /// 新增一个空白服务器条目并激活，返回其 id。
    /// 地址与用户名都为空时 id 会与默认条目碰撞，因此带上递增编号区分。
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

    /// 删除指定服务器；只删剩下的最后一条时拒绝（返回 false）。
    /// 删除当前激活条目时自动切到相邻条目。
    pub fn remove_server(&mut self, id: &str) -> bool {
        if self.servers.len() <= 1 {
            return false;
        }
        let Some(index) = self.servers.iter().position(|profile| profile.id == id) else {
            return false;
        };
        let was_active = self.active_server.as_deref() == Some(id);
        self.servers.remove(index);
        if was_active {
            let next = self.servers[index.min(self.servers.len() - 1)].id.clone();
            self.switch_to(&next);
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        locale_from_os_tag, normalize_queue_origins, AlbumSort, ArtistSort, CapabilitySupport,
        Config, LanguagePreference, ListeningHistory, OpenSubsonicExtension, PlayModes,
        PlaybackMode, PlaybackSession, QueueOrigin, RepeatMode, ServerCapabilities, ServerInfo,
        ServerProfile, ShortcutAction, Song, ThemePreference, TranscodingQuality,
        VolumeNormalization, LISTENING_HISTORY_LIMIT,
    };

    #[test]
    fn listening_history_tracks_attempts_updates_and_recent_unique_ids() {
        let song = |id: &str| Song {
            id: id.into(),
            title: id.into(),
            ..Song::default()
        };
        let mut history = ListeningHistory::default();
        history.record_started(song(""), 1);
        history.record_started(song("a"), 10);
        history.record_started(song("b"), 20);
        history.record_started(song("a"), 30);

        assert_eq!(history.events.len(), 3);
        assert!(history.update_latest("a", 42, false));
        assert!(history.update_latest("a", 12, true));
        assert!(!history.update_latest("missing", 9, true));
        assert_eq!(history.events[0].listened_seconds, 0);
        assert_eq!(history.events[2].listened_seconds, 42);
        assert!(history.events[2].completed);
        assert_eq!(
            history
                .recent_songs(10)
                .iter()
                .map(|song| song.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b", "a"]
        );
        assert_eq!(
            history.recent_song_ids(10),
            std::collections::HashSet::from(["a".to_string(), "b".to_string()])
        );
        assert!(history.recent_song_ids(0).is_empty());
    }

    #[test]
    fn listening_history_discards_the_oldest_entries_at_its_limit() {
        let mut history = ListeningHistory::default();
        for index in 0..=LISTENING_HISTORY_LIMIT {
            history.record_started(
                Song {
                    id: format!("song-{index}"),
                    ..Song::default()
                },
                index as u64,
            );
        }
        assert_eq!(history.events.len(), LISTENING_HISTORY_LIMIT);
        assert_eq!(history.events[0].song.id, "song-1");
        assert_eq!(
            history.events.last().map(|event| event.song.id.as_str()),
            Some("song-1000")
        );
    }

    #[test]
    fn capability_matrix_distinguishes_supported_unsupported_and_unknown() {
        let discovered = ServerInfo {
            protocol_version: "1.16.1".into(),
            open_subsonic: true,
            extensions_discovered: true,
            extensions: vec![
                OpenSubsonicExtension {
                    name: "songLyrics".into(),
                    versions: vec![1, 2],
                },
                OpenSubsonicExtension {
                    name: "sonicSimilarity".into(),
                    versions: vec![1],
                },
                OpenSubsonicExtension {
                    name: "playbackReport".into(),
                    versions: vec![1],
                },
            ],
            ..ServerInfo::default()
        };
        let capabilities = ServerCapabilities::from_server_info(&discovered);

        assert_eq!(capabilities.structured_lyrics, CapabilitySupport::Supported);
        assert_eq!(capabilities.enhanced_lyrics, CapabilitySupport::Supported);
        assert_eq!(capabilities.sonic_similarity, CapabilitySupport::Supported);
        assert_eq!(
            capabilities.top_songs_by_artist_id,
            CapabilitySupport::Unsupported
        );
        assert_eq!(capabilities.playback_report, CapabilitySupport::Supported);
        assert_eq!(
            capabilities.index_based_queue,
            CapabilitySupport::Unsupported
        );
        assert_eq!(capabilities.internet_radio, CapabilitySupport::Supported);
        assert_eq!(capabilities.podcasts, CapabilitySupport::Supported);
        assert_eq!(
            capabilities.podcast_episode_lookup,
            CapabilitySupport::Unsupported
        );
        assert_eq!(capabilities.smart_discovery(), CapabilitySupport::Supported);

        let compatibility = ServerInfo {
            protocol_version: "1.8.0".into(),
            ..ServerInfo::default()
        };
        let capabilities = ServerCapabilities::from_server_info(&compatibility);
        assert_eq!(capabilities.structured_lyrics, CapabilitySupport::Unknown);
        assert!(capabilities.structured_lyrics.allows_optimistic_request());
        assert_eq!(capabilities.internet_radio, CapabilitySupport::Unsupported);
        assert_eq!(capabilities.podcasts, CapabilitySupport::Supported);
    }

    #[test]
    fn invalid_protocol_versions_leave_base_capabilities_unknown() {
        let info = ServerInfo {
            protocol_version: "unknown".into(),
            ..ServerInfo::default()
        };
        let capabilities = ServerCapabilities::from_server_info(&info);

        assert_eq!(capabilities.internet_radio, CapabilitySupport::Unknown);
        assert_eq!(capabilities.podcasts, CapabilitySupport::Unknown);
    }

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
        // 新增的桌面行为字段带默认值，旧配置无需迁移即可加载。
        assert!(config.show_track_notifications);
        assert!(!config.close_minimizes);
        assert_eq!(
            config.shortcuts.binding(ShortcutAction::TogglePlayback),
            "space"
        );
        assert_eq!(
            config
                .global_shortcuts
                .binding(ShortcutAction::TogglePlayback),
            "ctrl-alt-space"
        );
    }

    #[test]
    fn partial_shortcut_config_falls_back_to_new_action_defaults() {
        let config: Config = serde_json::from_str(
            r#"{
                "server_url":"http://localhost:4533",
                "username":"user",
                "password":"pass",
                "shortcuts":{"bindings":{"toggle_playback":"ctrl-p"}}
            }"#,
        )
        .expect("partial shortcut config should deserialize");

        assert_eq!(
            config.shortcuts.binding(ShortcutAction::TogglePlayback),
            "ctrl-p"
        );
        assert_eq!(config.shortcuts.binding(ShortcutAction::Next), "ctrl-right");
    }

    #[test]
    fn partial_global_shortcut_config_falls_back_to_safe_defaults() {
        let config: Config = serde_json::from_str(
            r#"{
                "server_url":"http://localhost:4533",
                "username":"user",
                "password":"pass",
                "global_shortcuts":{"bindings":{"toggle_playback":"win-p"}}
            }"#,
        )
        .expect("partial global shortcut config should deserialize");

        assert_eq!(
            config
                .global_shortcuts
                .binding(ShortcutAction::TogglePlayback),
            "win-p"
        );
        assert_eq!(
            config.global_shortcuts.binding(ShortcutAction::Next),
            "ctrl-alt-right"
        );
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
    fn auto_continue_is_opt_in_and_round_trips() {
        let legacy: Config = serde_json::from_str(
            r#"{"server_url":"http://localhost:4533","username":"user","password":"pass"}"#,
        )
        .expect("legacy config without auto continue should still deserialize");
        assert!(!legacy.auto_continue);

        let json = serde_json::to_string(&Config {
            auto_continue: true,
            ..Config::default()
        })
        .expect("config should serialize");
        let restored: Config = serde_json::from_str(&json).expect("config should deserialize");
        assert!(restored.auto_continue);
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
    fn interface_language_round_trips_and_legacy_configs_follow_the_system() {
        let config = Config {
            language: LanguagePreference::ChineseTraditional,
            ..Config::default()
        };
        let json = serde_json::to_string(&config).expect("config should serialize");
        let restored: Config = serde_json::from_str(&json).expect("config should deserialize");
        assert_eq!(restored.language, LanguagePreference::ChineseTraditional);
        assert!(json.contains(r#""language":"chinese_traditional""#));

        // 旧配置没有 language 字段：默认 System（跟随操作系统），不能报错。
        let legacy: Config = serde_json::from_str(
            r#"{"server_url":"http://localhost:4533","username":"user","password":"pass"}"#,
        )
        .expect("legacy config without a language should still deserialize");
        assert_eq!(legacy.language, LanguagePreference::System);
        assert_eq!(legacy.language.locale_code(), "system");
    }

    #[test]
    fn os_language_tags_resolve_to_a_supported_catalog() {
        for tag in ["en", "en-US", "de-DE", "ja-JP", "", "  "] {
            assert_eq!(locale_from_os_tag(tag), "en", "tag={tag:?}");
        }
        for tag in ["zh", "zh-CN", "zh-Hans", "zh-Hans-CN", "zh-SG", " ZH-cn "] {
            assert_eq!(locale_from_os_tag(tag), "zh-CN", "tag={tag:?}");
        }
        for tag in [
            "zh-TW",
            "zh-HK",
            "zh-MO",
            "zh-Hant",
            "zh-Hant-TW",
            "zh-hant-hk",
        ] {
            assert_eq!(locale_from_os_tag(tag), "zh-TW", "tag={tag:?}");
        }
        // 显式选择的语言不受操作系统影响。
        assert_eq!(LanguagePreference::English.resolve(), "en");
        assert_eq!(LanguagePreference::ChineseSimplified.resolve(), "zh-CN");
        assert_eq!(LanguagePreference::ChineseTraditional.resolve(), "zh-TW");
        // 语言按钮用各自的母语自称，不随界面语言变化。
        assert_eq!(
            LanguagePreference::ChineseSimplified.native_label(),
            "简体中文"
        );
        assert_eq!(
            LanguagePreference::ChineseTraditional.native_label(),
            "繁體中文"
        );
        assert_eq!(LanguagePreference::English.native_label(), "English");
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
            queue_origins: vec![QueueOrigin::DailyMix],
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
        assert_eq!(restored.queue_origins, vec![QueueOrigin::DailyMix]);
        assert_eq!(restored.queue.len(), 1);
        assert_eq!(restored.queue_index, Some(2));
        assert!((restored.position_secs - 61.5).abs() < 1e-9);
        assert!(restored.playback_modes.shuffle);
        assert_eq!(restored.playback_modes.repeat, RepeatMode::All);
        assert_eq!(restored.shuffle_played, vec![0, 3, 1]);
        assert_eq!(restored.shuffle_forward, vec![2]);
    }

    #[test]
    fn legacy_queue_origins_default_to_manual_and_length_is_normalized() {
        let legacy = r#"{
            "server_url":"http://localhost:4533",
            "queue":[{"id":"a","title":"A"},{"id":"b","title":"B"}],
            "queue_index":0,
            "position_secs":0,
            "playback_modes":{"shuffle":false,"repeat":"off"},
            "shuffle_played":[],
            "shuffle_forward":[]
        }"#;
        let session: PlaybackSession = serde_json::from_str(legacy).unwrap();
        assert!(session.queue_origins.is_empty());
        assert_eq!(
            normalize_queue_origins(session.queue_origins, session.queue.len()),
            vec![QueueOrigin::Manual, QueueOrigin::Manual]
        );
        assert_eq!(
            normalize_queue_origins(vec![QueueOrigin::SongRadio, QueueOrigin::DailyMix], 1),
            vec![QueueOrigin::SongRadio]
        );
    }

    #[test]
    fn corrupted_session_json_fails_cleanly() {
        assert!(serde_json::from_str::<PlaybackSession>("not json at all").is_err());
    }

    #[test]
    fn legacy_single_server_config_migrates_into_the_server_list() {
        let mut config: Config = serde_json::from_str(
            r#"{"server_url":"http://nas.local:4533","username":"alice","password":"secret"}"#,
        )
        .expect("legacy config should deserialize");
        assert!(config.servers.is_empty(), "legacy config has no list yet");

        config.sync_active_profile();

        assert_eq!(config.servers.len(), 1);
        let profile = config.servers.first().unwrap();
        assert_eq!(profile.url, "http://nas.local:4533");
        assert_eq!(profile.username, "alice");
        // 遗留字段保持为激活服务器的镜像，旧调用点无需改动。
        assert_eq!(config.server_url, profile.url);
        assert_eq!(config.active_server.as_deref(), Some(profile.id.as_str()));
    }

    #[test]
    fn server_ids_are_stable_and_independent_of_case_and_trailing_slash() {
        let a = ServerProfile::id_for("http://NAS.local:4533/", "alice");
        let b = ServerProfile::id_for("  http://nas.local:4533  ", " alice ");
        assert_eq!(a, b);
        assert_eq!(a.len(), 12);
        assert!(a.chars().all(|ch| ch.is_ascii_hexdigit()));
        // 不同的用户名/端口必须区分开，否则缓存会串服务器。
        assert_ne!(a, ServerProfile::id_for("http://nas.local:4533", "bob"));
        assert_ne!(a, ServerProfile::id_for("http://nas.local:4534", "alice"));
    }

    #[test]
    fn switching_servers_mirrors_the_active_profile_into_legacy_fields() {
        let mut config = Config {
            servers: vec![
                ServerProfile::new("http://one.local:4533", "alice", "pw1"),
                ServerProfile::new("http://two.local:4533", "bob", "pw2"),
            ],
            ..Config::default()
        };
        config.sync_active_profile();
        let second = config.servers[1].id.clone();

        assert!(config.switch_to(&second));
        assert_eq!(config.server_url, "http://two.local:4533");
        assert_eq!(config.username, "bob");
        assert_eq!(config.password, "pw2");

        // 未知 id 不改变任何状态。
        assert!(!config.switch_to("does-not-exist"));
        assert_eq!(config.username, "bob");
    }

    #[test]
    fn adding_a_server_activates_a_blank_entry_without_colliding_ids() {
        let mut config = Config::default();
        config.sync_active_profile();
        let first = config.active_server.clone().unwrap();

        let added = config.add_server();
        assert_ne!(added, first);
        assert_eq!(config.servers.len(), 2);
        assert_eq!(config.active_server.as_deref(), Some(added.as_str()));
        // 新条目是空白条目，等待用户填写。
        assert!(config.server_url.is_empty());
        assert!(config.username.is_empty());
        assert!(!config.active_profile().unwrap().is_configured());

        // 连续新增不能产生重复 id。
        let another = config.add_server();
        assert_eq!(
            config
                .servers
                .iter()
                .filter(|profile| profile.id == another)
                .count(),
            1
        );
    }

    #[test]
    fn removing_the_active_server_falls_back_to_a_neighbour() {
        let mut config = Config {
            servers: vec![
                ServerProfile::new("http://one.local:4533", "alice", "pw1"),
                ServerProfile::new("http://two.local:4533", "bob", "pw2"),
            ],
            ..Config::default()
        };
        config.sync_active_profile();
        let first = config.servers[0].id.clone();
        let second = config.servers[1].id.clone();
        config.switch_to(&first);

        assert!(config.remove_server(&first));
        assert_eq!(config.servers.len(), 1);
        assert_eq!(config.active_server.as_deref(), Some(second.as_str()));
        assert_eq!(config.server_url, "http://two.local:4533");

        // 最后一条不能删：否则会退化成没有服务器可连的状态。
        assert!(!config.remove_server(&second));
        assert_eq!(config.servers.len(), 1);
    }

    #[test]
    fn editing_the_active_server_keeps_its_id_so_caches_survive_a_url_edit() {
        let mut config = Config {
            servers: vec![ServerProfile::new("http://old.local:4533", "alice", "pw1")],
            ..Config::default()
        };
        config.sync_active_profile();
        let original_id = config.active_server.clone().unwrap();

        let id = config.store_active("http://new.local:4533", "alice", "pw2");

        assert_eq!(id, original_id, "改地址不应换缓存命名空间");
        assert_eq!(config.servers.len(), 1);
        assert_eq!(config.server_url, "http://new.local:4533");
        assert_eq!(config.password, "pw2");
    }

    #[test]
    fn storing_a_known_peer_identity_merges_instead_of_duplicating() {
        let mut config = Config {
            servers: vec![
                ServerProfile::new("http://one.local:4533", "alice", "pw1"),
                ServerProfile::new("http://two.local:4533", "bob", "pw2"),
            ],
            ..Config::default()
        };
        config.sync_active_profile();
        let second = config.servers[1].id.clone();
        config.switch_to(&second);

        // 在第二条条目里填上第一条的地址+账号：应合并回第一条，而不是新建。
        let id = config.store_active("http://one.local:4533", "alice", "pw-new");

        assert_eq!(config.servers.len(), 2);
        assert_eq!(id, config.servers[0].id);
        assert_eq!(config.active_server.as_deref(), Some(id.as_str()));
        assert_eq!(config.servers[0].password, "pw-new");
    }

    #[test]
    fn storing_a_non_active_profile_updates_it_in_place_without_switching() {
        let mut config = Config {
            servers: vec![
                ServerProfile::new("http://one.local:4533", "alice", "pw1"),
                ServerProfile::new("http://two.local:4533", "bob", "pw2"),
            ],
            ..Config::default()
        };
        config.sync_active_profile();
        assert_eq!(
            config.active_server.as_deref(),
            Some(config.servers[0].id.as_str())
        );
        let second_id = config.servers[1].id.clone();

        // 行内铅笔编辑非激活条目：原位更新，id 与激活项都不变。
        assert!(config.store_profile(
            &second_id,
            "  http://two.local:4533/new  ",
            "  robert ",
            "pw-new"
        ));

        let second = &config.servers[1];
        assert_eq!(second.id, second_id, "条目 id 不能变，缓存命名空间依赖它");
        assert_eq!(second.url, "http://two.local:4533/new");
        assert_eq!(second.username, "robert");
        assert_eq!(second.password, "pw-new");
        assert!(!second.password_in_keyring);
        assert_eq!(
            config.active_server.as_deref(),
            Some(config.servers[0].id.as_str()),
            "编辑非激活条目不能把连接切走"
        );

        // 目标 id 不存在（刚被删除）：安全失败，不动任何条目。
        assert!(!config.store_profile("missing", "http://x", "y", "z"));
    }

    #[test]
    fn server_label_falls_back_to_host_when_unnamed() {
        let profile = ServerProfile::new("http://192.168.1.10:4533", "alice", "pw");
        assert_eq!(profile.host(), "192.168.1.10:4533");
        assert_eq!(profile.label(), "192.168.1.10:4533");

        let named = ServerProfile {
            name: "  Home NAS  ".to_string(),
            ..profile
        };
        assert_eq!(named.label(), "Home NAS");
    }

    #[test]
    fn keyring_flag_defaults_off_for_legacy_configs() {
        let config: Config = serde_json::from_str(
        r#"{"server_url":"http://nas.local:4533","username":"alice","password":"secret","servers":[{"id":"abc","url":"http://nas.local:4533","username":"alice","password":"secret"}]}"#,
    )
    .expect("legacy server entries should deserialize without the keyring flag");
        assert!(!config.servers[0].password_in_keyring);

        // 序列化时必须写出该标记，供回环保持。
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("password_in_keyring"));
    }

    #[test]
    fn resolved_password_prefers_the_credential_store_and_falls_back_to_plaintext() {
        let profile = ServerProfile::new("http://nas.local:4533", "alice", "pw");
        assert_eq!(profile.resolved_password(), "pw");

        // 凭据库里没有这个 id 的条目（测试专用 id）：即使被误标为已迁入，
        // 也必须回退明文字段，绝不能因为读取失败而丢密码。
        let flagged = ServerProfile {
            password_in_keyring: true,
            ..profile
        };
        assert_eq!(flagged.resolved_password(), "pw");
    }

    #[test]
    fn opus_sources_fall_back_to_server_transcoding_only_at_original_quality() {
        // Original + Opus：自动降级到既有档位里最高的 320 kbps。
        assert_eq!(
            TranscodingQuality::effective_for(
                TranscodingQuality::Original,
                Some("opus"),
                Some("audio/ogg")
            ),
            TranscodingQuality::transcode_fallback()
        );
        // 判定容忍大小写与空白；content type audio/opus 单独成立。
        assert!(TranscodingQuality::is_undecodable_source(
            Some(" OPUS "),
            None
        ));
        assert!(TranscodingQuality::is_undecodable_source(
            None,
            Some("Audio/Opus")
        ));

        // 已选转码档位：服务器本来就会转码，保持用户选择不变。
        assert_eq!(
            TranscodingQuality::effective_for(TranscodingQuality::Kbps128, Some("opus"), None),
            TranscodingQuality::Kbps128
        );

        // Vorbis / FLAC / 未知后缀都保持 Original：只有 Opus 才降级，
        // 尤其不能把 Ogg Vorbis（可正常解码）一并误伤。
        for suffix in ["ogg", "oga", "flac", "mp3", "m4a", "wma"] {
            assert!(!TranscodingQuality::is_undecodable_source(
                Some(suffix),
                None
            ));
            assert_eq!(
                TranscodingQuality::effective_for(TranscodingQuality::Original, Some(suffix), None),
                TranscodingQuality::Original
            );
        }

        // 两者都缺失时不盲目降级（无从判定，保持现状）。
        assert!(!TranscodingQuality::is_undecodable_source(None, None));
        assert_eq!(
            TranscodingQuality::effective_for(TranscodingQuality::Original, None, None),
            TranscodingQuality::Original
        );
    }
}
