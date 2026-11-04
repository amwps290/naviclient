//! 应用运行时状态、内部状态类型及默认初始化。

use super::*;

/// 歌曲列表多选状态：作用域 + Shift 范围选择锚点 + 升序索引集合。
/// 锚点是范围选择的固定端点（Ctrl+点击或首个范围操作的落点），
/// Shift+点击 / Shift+↑↓ 都从锚点重新计算范围。
#[derive(Debug, Clone)]
pub(crate) struct SongListMulti {
    pub(super) scope: &'static str,
    pub(super) anchor: usize,
    pub(super) indices: std::collections::BTreeSet<usize>,
}

/// 重连后需要前进这么多才认为真的恢复，避免刚发出 Play 就清零计数。
pub(super) const RECOVERY_CONFIRMATION: Duration = Duration::from_secs(5);

/// 一次自动重连计划：到点后从 `position` 重新拉流。
pub(super) struct Reconnect {
    pub(super) at: std::time::Instant,
    /// 已尝试次数（1 = 第一次重试）。
    pub(super) attempt: u8,
    pub(super) position: Duration,
    pub(super) song_id: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SettingsSection {
    #[default]
    Servers,
    Playback,
    Desktop,
    Appearance,
    Shortcuts,
}

pub(super) struct AppState {
    pub(super) server: Option<ServerInfo>,
    pub(super) view: View,
    pub(super) loading: bool,
    pub(super) artists: Vec<Artist>,
    pub(super) artists_visible: usize,
    pub(super) artists_sort: ArtistSort,
    pub(super) albums: Vec<Album>,
    pub(super) albums_loading: bool,
    pub(super) albums_exhausted: bool,
    pub(super) albums_page: usize,
    /// 最近一次专辑分页请求失败时为 true：网格下方显示重试入口。
    pub(super) albums_page_failed: bool,
    pub(super) albums_sort: AlbumSort,
    pub(super) newest_albums: Vec<Album>,
    pub(super) newest_albums_loading: bool,
    pub(super) recent_albums: Vec<Album>,
    pub(super) recent_albums_loading: bool,
    pub(super) frequent_albums: Vec<Album>,
    pub(super) frequent_albums_loading: bool,
    pub(super) random_albums: Vec<Album>,
    pub(super) random_albums_loading: bool,
    pub(super) daily_mix: Option<DailyMixSnapshot>,
    /// 上一版（含跨日）每日精选的歌曲，用于提高换歌率。
    pub(super) daily_mix_previous_ids: HashSet<String>,
    pub(super) daily_mix_loading: bool,
    pub(super) daily_mix_error: Option<String>,
    pub(super) daily_mix_play_when_ready: bool,
    pub(super) genres: Vec<Genre>,
    pub(super) genres_loading: bool,
    pub(super) current_genre: Option<String>,
    pub(super) genre_albums: Vec<Album>,
    pub(super) genre_albums_loading: bool,
    pub(super) genre_albums_page: usize,
    pub(super) genre_albums_exhausted: bool,
    /// 最近一次流派专辑分页请求失败时为 true。
    pub(super) genre_albums_page_failed: bool,
    pub(super) search_filter: SearchFilter,
    pub(super) collapsed_sections: HashSet<String>,
    pub(super) current_artist: Option<Artist>,
    pub(super) artist_albums: Vec<Album>,
    pub(super) artist_top_songs: Vec<Song>,
    pub(super) artist_top_songs_loading: bool,
    pub(super) artist_top_songs_error: Option<String>,
    pub(super) current_album: Option<Album>,
    pub(super) current_songs: Vec<Song>,
    pub(super) playlists: Vec<Playlist>,
    pub(super) favorites: Favorites,
    pub(super) favorite_ids: HashSet<FavoriteKey>,
    pub(super) pending_favorites: HashSet<FavoriteKey>,
    pub(super) current_playlist: Option<Playlist>,
    pub(super) playlist_songs: Vec<Song>,
    pub(super) search_results: Option<SearchResults>,
    pub(super) playlists_visible: usize,
    pub(super) hovered_item: Option<String>,
    pub(super) pending_play_album: bool,
    pub(super) queue: Vec<Song>,
    /// 与 queue 按索引对齐；所有队列变更必须同步更新。
    pub(super) queue_origins: Vec<QueueOrigin>,
    pub(super) queue_index: Option<usize>,
    /// 右侧 Up Next / History 抽屉；不再用独立页面替换当前浏览上下文。
    pub(super) queue_drawer_open: bool,
    pub(super) queue_drawer_tab: QueueDrawerTab,
    /// 最近实际开始播放的歌曲，新到旧排列；保留重复项，不等同于队列前缀。
    pub(super) playback_history: PlaybackHistory,
    pub(super) now_playing: Option<Song>,
    pub(super) now_playing_quality: Option<TranscodingQuality>,
    pub(super) lyrics: Option<Lyrics>,
    pub(super) lyrics_song_id: Option<String>,
    pub(super) lyrics_loading: bool,
    pub(super) lyrics_error: Option<String>,
    pub(super) covers: HashMap<String, Arc<GpuiImage>>,
    pub(super) cover_palettes: HashMap<String, (Hsla, Hsla)>,
    pub(super) requested_covers: HashSet<String>,
    pub(super) failed_covers: HashSet<String>,
    pub(super) failed_cover_order: VecDeque<String>,
    pub(super) status: String,
    pub(super) settings_open: bool,
    /// 设置页左侧导航当前选中的分组。
    pub(super) settings_section: SettingsSection,
    /// 设置页正在等待用户按下组合键的快捷键动作。
    pub(super) recording_shortcut: Option<ShortcutAction>,
    /// 最近一次录入与另一个动作冲突：（正在编辑，已占用动作）。
    pub(super) shortcut_conflict: Option<(ShortcutAction, ShortcutAction)>,
    /// 设置页正在等待用户按下系统全局组合键的播放动作。
    pub(super) recording_global_shortcut: Option<ShortcutAction>,
    /// 最近一次全局组合键与另一个全局动作冲突。
    pub(super) global_shortcut_conflict: Option<(ShortcutAction, ShortcutAction)>,
    /// 最近一次全局组合键录入未通过安全性/键位校验。
    pub(super) global_shortcut_validation: Option<(ShortcutAction, GlobalShortcutValidationError)>,
    /// Windows 注册失败的动作（通常是组合键已被其他程序占用）。
    pub(super) global_shortcut_failures: HashSet<ShortcutAction>,
    /// 设置页当前正在编辑的服务器条目 id（行内铅笔按钮打开）。
    /// None = 没有展开的编辑表单；未配置的激活条目（首次运行/刚新增）
    /// 无需显式目标也会直出表单。保存或切换/删除服务器会经
    /// reconnect 重置 state，表单随之收起。
    pub(super) editing_server: Option<String>,
    /// 键盘导航选中的歌曲行：作用域 + 行号。
    pub(super) song_list_selected: Option<(&'static str, usize)>,
    /// 歌曲列表多选集合。索引只在该 scope 当前列表内有效；列表数据
    /// 变化处负责清空（见 clear_song_multi）。
    pub(super) song_list_multi: Option<SongListMulti>,
    /// “曲目信息”对话框当前展示的歌曲。
    pub(super) song_info: Option<Song>,
    pub(super) view_before_now_playing: View,
    pub(super) playback_modes: PlayModes,
    pub(super) shuffle_history: ShuffleHistory,
    pub(super) ended_handled: bool,
    pub(super) now_playing_reported: Option<String>,
    pub(super) scrobbled_song_id: Option<String>,
    /// 音频输出设备清单（设置页展示）：(名称, 是否默认)。
    pub(super) output_devices: Vec<(String, bool)>,
    pub(super) output_devices_loading: bool,
    /// 本地收听历史；与播放会话一样按服务器隔离。
    pub(super) listening_history: ListeningHistory,
    /// 本地测量的歌曲响度缓存：song_id → (rms_db, peak)。
    pub(super) loudness_cache: HashMap<String, (f32, f32)>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            server: None,
            view: View::Home,
            loading: false,
            artists: Vec::new(),
            artists_visible: 0,
            artists_sort: ArtistSort::default(),
            albums: Vec::new(),
            albums_loading: false,
            albums_exhausted: false,
            albums_page: 0,
            albums_page_failed: false,
            albums_sort: AlbumSort::default(),
            newest_albums: Vec::new(),
            newest_albums_loading: false,
            recent_albums: Vec::new(),
            recent_albums_loading: false,
            frequent_albums: Vec::new(),
            frequent_albums_loading: false,
            random_albums: Vec::new(),
            random_albums_loading: false,
            daily_mix: None,
            daily_mix_previous_ids: HashSet::new(),
            daily_mix_loading: false,
            daily_mix_error: None,
            daily_mix_play_when_ready: false,
            genres: Vec::new(),
            genres_loading: false,
            current_genre: None,
            genre_albums: Vec::new(),
            genre_albums_loading: false,
            genre_albums_page: 0,
            genre_albums_exhausted: false,
            genre_albums_page_failed: false,
            search_filter: SearchFilter::default(),
            collapsed_sections: HashSet::from(["random".to_string()]),
            current_artist: None,
            artist_albums: Vec::new(),
            artist_top_songs: Vec::new(),
            artist_top_songs_loading: false,
            artist_top_songs_error: None,
            current_album: None,
            current_songs: Vec::new(),
            playlists: Vec::new(),
            favorites: Favorites::default(),
            favorite_ids: HashSet::new(),
            pending_favorites: HashSet::new(),
            current_playlist: None,
            playlist_songs: Vec::new(),
            search_results: None,
            playlists_visible: 50,
            hovered_item: None,
            pending_play_album: false,
            queue: Vec::new(),
            queue_origins: Vec::new(),
            queue_index: None,
            queue_drawer_open: false,
            queue_drawer_tab: QueueDrawerTab::default(),
            playback_history: PlaybackHistory::default(),
            now_playing: None,
            now_playing_quality: None,
            lyrics: None,
            lyrics_song_id: None,
            lyrics_loading: false,
            lyrics_error: None,
            covers: HashMap::new(),
            cover_palettes: HashMap::new(),
            requested_covers: HashSet::new(),
            failed_covers: HashSet::new(),
            failed_cover_order: VecDeque::new(),
            status: t!("Not connected").to_string(),
            settings_open: false,
            settings_section: SettingsSection::default(),
            recording_shortcut: None,
            shortcut_conflict: None,
            recording_global_shortcut: None,
            global_shortcut_conflict: None,
            global_shortcut_validation: None,
            global_shortcut_failures: HashSet::new(),
            editing_server: None,
            song_list_selected: None,
            song_list_multi: None,
            song_info: None,
            view_before_now_playing: View::Home,
            playback_modes: PlayModes::default(),
            shuffle_history: ShuffleHistory::default(),
            ended_handled: true,
            now_playing_reported: None,
            scrobbled_song_id: None,
            output_devices: Vec::new(),
            output_devices_loading: false,
            listening_history: ListeningHistory::default(),
            loudness_cache: HashMap::new(),
        }
    }
}
