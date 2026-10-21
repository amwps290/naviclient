use std::path::PathBuf;

use md5::{Digest, Md5};
use rust_i18n::t;
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
    /// 展示名（翻译键）。
    pub fn label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Light => t!("Light"),
            Self::Dark => t!("Dark"),
            Self::System => t!("System"),
        }
    }
}

/// 界面语言偏好。`System` 跟随操作系统：Windows 经 `sys-locale` 检测，
/// 中文环境自动落到简体，繁体区域（TW/HK/MO/Hant）落到繁体；
/// 检测失败时回退英文。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguagePreference {
    #[default]
    System,
    English,
    ChineseSimplified,
    ChineseTraditional,
}

impl LanguagePreference {
    pub const ALL: [Self; 4] = [
        Self::System,
        Self::English,
        Self::ChineseSimplified,
        Self::ChineseTraditional,
    ];

    /// rust-i18n 的 locale 代码；`System` 在这里返回哨兵值，
    /// 实际生效代码用 [`Self::resolve`]。
    pub fn locale_code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::English => "en",
            Self::ChineseSimplified => "zh-CN",
            Self::ChineseTraditional => "zh-TW",
        }
    }

    /// 设置页按钮上的自称名：语言用自己的名字（英文不译、中文用中文原文），
    /// `System` 走翻译键（跟随当前界面语言）。
    pub fn native_label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::System => rust_i18n::t!("System"),
            Self::English => std::borrow::Cow::Borrowed("English"),
            Self::ChineseSimplified => std::borrow::Cow::Borrowed("简体中文"),
            Self::ChineseTraditional => std::borrow::Cow::Borrowed("繁體中文"),
        }
    }

    /// 解析成实际生效的 locale 代码（`System` 探测操作系统语言）。
    pub fn resolve(self) -> &'static str {
        match self {
            Self::System => locale_from_os_tag(&sys_locale::get_locale().unwrap_or_default()),
            other => other.locale_code(),
        }
    }
}

/// 由操作系统语言标签推断本应用支持的 locale。
/// 纯函数便于测试：zh-TW/HK/MO/Hant 系繁体，其余 zh 简体，其他英文。
pub fn locale_from_os_tag(tag: &str) -> &'static str {
    let lower = tag.trim().to_lowercase();
    if !lower.starts_with("zh") {
        return "en";
    }
    let traditional = ["zh-tw", "zh-hk", "zh-mo", "zh-hant", "zh-tw_"]
        .iter()
        .any(|prefix| lower.starts_with(prefix) || lower.contains("hant"));
    if traditional {
        "zh-TW"
    } else {
        "zh-CN"
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

    /// 展示名（kbps 档位是技术记号，无对应翻译时回退原文）。
    pub fn label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Original => t!("Original"),
            Self::Kbps128 => t!("128 kbps"),
            Self::Kbps192 => t!("192 kbps"),
            Self::Kbps256 => t!("256 kbps"),
            Self::Kbps320 => t!("320 kbps"),
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

    /// 判断音源是否为本地解码链不支持的编码（Ogg Opus）。
    ///
    /// symphonia 0.5 尚未发布 Opus 解码器，rodio 也没有对应 feature；
    /// Original 档位拿到原始 Opus 流会在解码器初始化时失败（报错文案
    /// 是误导性的“格式未识别”）。判定依据歌曲元数据的 suffix 与
    /// content type，均做 trim 并忽略大小写；两者都缺失时按可解码处理
    /// （保持现状，不盲目降级）。
    pub fn is_undecodable_source(suffix: Option<&str>, content_type: Option<&str>) -> bool {
        let is = |text: Option<&str>, expected: &str| {
            text.is_some_and(|text| text.trim().eq_ignore_ascii_case(expected))
        };
        is(suffix, "opus") || is(content_type, "audio/opus")
    }

    /// 取流实际使用的音质：Original 遇到本地无法解码的 Opus 源时，
    /// 自动降级为服务端转码，取既有档位里最高的 320 kbps，尽量贴近
    /// “原始音质”的意图；其余情况（含已选转码档位）保持配置不变——
    /// 用户手动选的转码档位本身就会触发服务端转码，无需干预。
    pub fn effective_for(
        configured: Self,
        suffix: Option<&str>,
        content_type: Option<&str>,
    ) -> Self {
        if configured == Self::Original && Self::is_undecodable_source(suffix, content_type) {
            Self::transcode_fallback()
        } else {
            configured
        }
    }

    /// Opus 自动降级的目标档位。
    pub const fn transcode_fallback() -> Self {
        Self::Kbps320
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

    pub fn tooltip(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Off => t!("Repeat off"),
            Self::All => t!("Repeat all"),
            Self::One => t!("Repeat current track"),
        }
    }
}

/// 均衡器设置：10 段图形均衡（31.25 Hz…16 kHz），每段 -12..+12 dB；
/// 关闭时音频直通。设置由音频工作线程在链路中应用（ReplayGain 增益
/// 之后、输出归一化之前），不影响响度测量。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EqSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub gains_db: [f32; 10],
}

impl Default for EqSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            gains_db: [0.0; 10],
        }
    }
}

impl EqSettings {
    /// 10 段中心频率（Hz，1/3 十倍频程排列）。
    pub const BANDS: [f32; 10] = [
        31.25, 62.5, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
    ];

    /// 内置预设：(名称, 10 段增益 dB)。
    pub const PRESETS: [(&'static str, [f32; 10]); 8] = [
        ("Flat", [0.0; 10]),
        (
            "Bass boost",
            [7.0, 6.0, 4.5, 3.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ),
        (
            "Treble boost",
            [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.5, 5.0, 6.5],
        ),
        (
            "Vocal",
            [-2.0, -1.5, 0.0, 2.0, 4.0, 4.0, 3.0, 1.5, 0.0, -1.0],
        ),
        ("Rock", [5.0, 4.0, 3.0, 1.0, -0.5, -1.0, 0.5, 2.5, 3.5, 4.0]),
        ("Pop", [-1.0, 0.5, 3.0, 4.0, 3.0, 0.5, -1.0, -1.0, 0.0, 1.0]),
        ("Jazz", [4.0, 3.0, 1.0, 2.0, -1.0, -1.0, 0.0, 1.0, 2.5, 3.5]),
        (
            "Classical",
            [4.0, 3.0, 2.0, 0.0, -1.0, -1.0, 0.0, 2.5, 3.5, 4.5],
        ),
    ];

    /// 预设的显示名。`PRESETS` 里的名称同时充当元素 ID 与配置无关的
    /// 稳定标识，因此只在显示时翻译（与侧边栏入口同一套做法）。
    pub fn preset_label(name: &'static str) -> std::borrow::Cow<'static, str> {
        match name {
            "Flat" => t!("Flat"),
            "Bass boost" => t!("Bass boost"),
            "Treble boost" => t!("Treble boost"),
            "Vocal" => t!("Vocal"),
            "Rock" => t!("Rock"),
            "Pop" => t!("Pop"),
            "Jazz" => t!("Jazz"),
            "Classical" => t!("Classical"),
            other => std::borrow::Cow::Borrowed(other),
        }
    }

    /// 把可能越界的滑杆值夹回 [-12, 12] dB。
    pub fn clamp_gains(&mut self) {
        for gain in &mut self.gains_db {
            *gain = gain.clamp(-12.0, 12.0);
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
    pub fn label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::NewestAdded => t!("Recently added"),
            Self::Name => t!("Name"),
            Self::Artist => t!("Artist"),
            Self::Year => t!("Year"),
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
    pub fn label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Name => t!("Name (A–Z)"),
            Self::NameDesc => t!("Name (Z–A)"),
            Self::AlbumCount => t!("Album count"),
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
    /// 保留为独立字段是为了兼容旧配置与 `Api::new` 等现有调用点；
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
            volume_normalization: VolumeNormalization::Track,
            sidebar_collapsed: false,
            show_lyrics_translation: default_true(),
            output_device: None,
            equalizer: EqSettings::default(),
            albums_sort: AlbumSort::default(),
            artists_sort: ArtistSort::default(),
            show_track_notifications: default_true(),
            close_minimizes: false,
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

    pub fn label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Off => t!("Off"),
            Self::Track => t!("Track"),
            Self::Album => t!("Album"),
        }
    }

    /// 说明文案（翻译键）；取词时点跟随当前界面语言。
    pub fn tooltip(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Off => t!("Play every song at its recorded loudness"),
            Self::Track => t!("Normalize each track to a consistent loudness (ReplayGain)"),
            Self::Album => t!("Normalize by album so a record's relative dynamics are preserved"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        locale_from_os_tag, AlbumSort, ArtistSort, Config, LanguagePreference, PlayModes,
        PlaybackMode, PlaybackSession, RepeatMode, ServerProfile, Song, ThemePreference,
        TranscodingQuality, VolumeNormalization,
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
        // 新增的桌面行为字段带默认值，旧配置无需迁移即可加载。
        assert!(config.show_track_notifications);
        assert!(!config.close_minimizes);
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
