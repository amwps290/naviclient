use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::assets::IconName as KitIcon;
use gpui_kit::component::{
    box_shadow,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{ContextMenuExt, DropdownMenu},
    notification::{Notification, NotificationType},
    scroll::{ScrollableElement, Scrollbar},
    slider::{Slider, SliderEvent, SliderState, SliderValue},
    switch::Switch,
    tooltip::Tooltip,
    v_flex, window_paddings, ActiveTheme, Disableable, Icon, IconName, IconNamed, Selectable,
    Sizable, Theme, ThemeMode, TitleBar, WindowExt,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, ease_out_quint, hsla, img, linear_color_stop, linear_gradient, percentage, point, px,
    relative, rems, size, uniform_list, Animation, AnimationExt, App, AppContext, Context, Entity,
    FontWeight, Hsla, Image as GpuiImage, ImageFormat as GpuiImageFormat, InteractiveElement,
    IntoElement, Keystroke, MouseButton, ObjectFit, ParentElement, PathPromptOptions, Pixels,
    Render, ScrollHandle, ScrollWheelEvent, SharedString, Size, StatefulInteractiveElement, Styled,
    StyledImage, Subscription, Transformation, UniformListScrollHandle, Window, WindowControlArea,
};
use rand::{seq::SliceRandom, thread_rng};
use rust_i18n::t;
use smol::Timer;
use tokio::runtime::Runtime;

use crate::api::{format_duration, OpenSubsonicProvider};
use crate::assets::{AppIcon, PlayerIcon};
use crate::audio::{
    format_playback, AudioHandle, PlaybackState, StreamFailure, TrackTransition,
    PLAYBACK_RECONNECT_ATTEMPTS,
};
use crate::config;
use crate::global_shortcuts::{
    GlobalShortcutEvent, GlobalShortcutManager, ValidationError as GlobalShortcutValidationError,
};
use crate::media_session::{self, MediaCommand, MediaSession, MediaUpdate};
use crate::models::{
    normalize_queue_origins, Album, AlbumSort, Artist, ArtistSort, Config, EqSettings, FavoriteKey,
    FavoriteKind, Favorites, Genre, LanguagePreference, ListeningHistory, Lyrics, PlayModes,
    PlaybackSession, Playlist, QueueOrigin, RepeatMode, SearchResults, ServerInfo, ServerProfile,
    ShortcutAction, Song, ThemePreference, TranscodingQuality, VolumeNormalization,
};
use crate::msg::{error_message, Msg};
use crate::notify;
use crate::recommendation::DailyMixSnapshot;
use crate::secrets;
use crate::single_instance;
use crate::tray::{self, TrayCommand};
use crate::vinyl::VinylCover;

#[cfg(test)]
mod tests;

mod artwork_view;
mod content_view;
mod covers;
mod desktop;
mod helpers;
mod import;
mod library;
mod library_view;
mod lyrics;
mod media_controls;
mod playback;
mod playback_lifecycle;
mod player_view;
mod playlists;
mod queue;
mod queue_view;
mod recommendations;
mod render;
mod settings;
mod settings_view;
mod shell_view;
mod shortcuts;
mod song_view;
use self::import::{ImportStage, ImportUi};
use helpers::*;
use playlists::{PlaylistUi, SongMenuContext};

/// 全局视觉 token 修正：普通控件统一 8px，卡片/弹窗统一 12px；
/// 同时提高亮色主题的小号次要文字对比度。
///
/// 配置副本会被后续 `Theme::change` 重用，因此运行时切换深浅主题不会
/// 丢失圆角规范。深色主题颜色保持 Kit 默认，仅同步圆角。
pub fn adjust_light_theme_contrast(cx: &mut App) {
    let mode = {
        let theme = Theme::global_mut(cx);
        let mut light = (*theme.light_theme).clone();
        light.radius = Some(8);
        light.radius_lg = Some(12);
        light.colors.muted_foreground = Some("#525252".into());
        theme.light_theme = std::rc::Rc::new(light);

        let mut dark = (*theme.dark_theme).clone();
        dark.radius = Some(8);
        dark.radius_lg = Some(12);
        theme.dark_theme = std::rc::Rc::new(dark);
        theme.mode
    };
    Theme::change(mode, None, cx);
}

/// 歌曲列表多选状态：作用域 + Shift 范围选择锚点 + 升序索引集合。
/// 锚点是范围选择的固定端点（Ctrl+点击或首个范围操作的落点），
/// Shift+点击 / Shift+↑↓ 都从锚点重新计算范围。
#[derive(Debug, Clone)]
pub(crate) struct SongListMulti {
    scope: &'static str,
    anchor: usize,
    indices: std::collections::BTreeSet<usize>,
}

/// 重连后需要前进这么多才认为真的恢复，避免刚发出 Play 就清零计数。
const RECOVERY_CONFIRMATION: Duration = Duration::from_secs(5);

/// 一次自动重连计划：到点后从 `position` 重新拉流。
struct Reconnect {
    at: std::time::Instant,
    /// 已尝试次数（1 = 第一次重试）。
    attempt: u8,
    position: Duration,
    song_id: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SettingsSection {
    #[default]
    Servers,
    Playback,
    Desktop,
    Appearance,
    Shortcuts,
}

struct AppState {
    server: Option<ServerInfo>,
    view: View,
    loading: bool,
    artists: Vec<Artist>,
    artists_visible: usize,
    artists_sort: ArtistSort,
    albums: Vec<Album>,
    albums_loading: bool,
    albums_exhausted: bool,
    albums_page: usize,
    /// 最近一次专辑分页请求失败时为 true：网格下方显示重试入口。
    albums_page_failed: bool,
    albums_sort: AlbumSort,
    newest_albums: Vec<Album>,
    newest_albums_loading: bool,
    recent_albums: Vec<Album>,
    recent_albums_loading: bool,
    frequent_albums: Vec<Album>,
    frequent_albums_loading: bool,
    random_albums: Vec<Album>,
    random_albums_loading: bool,
    daily_mix: Option<DailyMixSnapshot>,
    /// 上一版（含跨日）每日精选的歌曲，用于提高换歌率。
    daily_mix_previous_ids: HashSet<String>,
    daily_mix_loading: bool,
    daily_mix_error: Option<String>,
    daily_mix_play_when_ready: bool,
    genres: Vec<Genre>,
    genres_loading: bool,
    current_genre: Option<String>,
    genre_albums: Vec<Album>,
    genre_albums_loading: bool,
    genre_albums_page: usize,
    genre_albums_exhausted: bool,
    /// 最近一次流派专辑分页请求失败时为 true。
    genre_albums_page_failed: bool,
    search_filter: SearchFilter,
    collapsed_sections: HashSet<String>,
    current_artist: Option<Artist>,
    artist_albums: Vec<Album>,
    artist_top_songs: Vec<Song>,
    artist_top_songs_loading: bool,
    artist_top_songs_error: Option<String>,
    current_album: Option<Album>,
    current_songs: Vec<Song>,
    playlists: Vec<Playlist>,
    favorites: Favorites,
    favorite_ids: HashSet<FavoriteKey>,
    pending_favorites: HashSet<FavoriteKey>,
    current_playlist: Option<Playlist>,
    playlist_songs: Vec<Song>,
    search_results: Option<SearchResults>,
    playlists_visible: usize,
    hovered_item: Option<String>,
    pending_play_album: bool,
    queue: Vec<Song>,
    /// 与 queue 按索引对齐；所有队列变更必须同步更新。
    queue_origins: Vec<QueueOrigin>,
    queue_index: Option<usize>,
    /// 右侧 Up Next / History 抽屉；不再用独立页面替换当前浏览上下文。
    queue_drawer_open: bool,
    queue_drawer_tab: QueueDrawerTab,
    /// 最近实际开始播放的歌曲，新到旧排列；保留重复项，不等同于队列前缀。
    playback_history: PlaybackHistory,
    now_playing: Option<Song>,
    now_playing_quality: Option<TranscodingQuality>,
    lyrics: Option<Lyrics>,
    lyrics_song_id: Option<String>,
    lyrics_loading: bool,
    lyrics_error: Option<String>,
    covers: HashMap<String, Arc<GpuiImage>>,
    cover_palettes: HashMap<String, (Hsla, Hsla)>,
    requested_covers: HashSet<String>,
    failed_covers: HashSet<String>,
    failed_cover_order: VecDeque<String>,
    status: String,
    settings_open: bool,
    /// 设置页左侧导航当前选中的分组。
    settings_section: SettingsSection,
    /// 设置页正在等待用户按下组合键的快捷键动作。
    recording_shortcut: Option<ShortcutAction>,
    /// 最近一次录入与另一个动作冲突：（正在编辑，已占用动作）。
    shortcut_conflict: Option<(ShortcutAction, ShortcutAction)>,
    /// 设置页正在等待用户按下系统全局组合键的播放动作。
    recording_global_shortcut: Option<ShortcutAction>,
    /// 最近一次全局组合键与另一个全局动作冲突。
    global_shortcut_conflict: Option<(ShortcutAction, ShortcutAction)>,
    /// 最近一次全局组合键录入未通过安全性/键位校验。
    global_shortcut_validation: Option<(ShortcutAction, GlobalShortcutValidationError)>,
    /// Windows 注册失败的动作（通常是组合键已被其他程序占用）。
    global_shortcut_failures: HashSet<ShortcutAction>,
    /// 设置页当前正在编辑的服务器条目 id（行内铅笔按钮打开）。
    /// None = 没有展开的编辑表单；未配置的激活条目（首次运行/刚新增）
    /// 无需显式目标也会直出表单。保存或切换/删除服务器会经
    /// reconnect 重置 state，表单随之收起。
    editing_server: Option<String>,
    /// 键盘导航选中的歌曲行：作用域 + 行号。
    song_list_selected: Option<(&'static str, usize)>,
    /// 歌曲列表多选集合。索引只在该 scope 当前列表内有效；列表数据
    /// 变化处负责清空（见 clear_song_multi）。
    song_list_multi: Option<SongListMulti>,
    /// “曲目信息”对话框当前展示的歌曲。
    song_info: Option<Song>,
    view_before_now_playing: View,
    playback_modes: PlayModes,
    shuffle_history: ShuffleHistory,
    ended_handled: bool,
    now_playing_reported: Option<String>,
    scrobbled_song_id: Option<String>,
    /// 音频输出设备清单（设置页展示）：(名称, 是否默认)。
    output_devices: Vec<(String, bool)>,
    output_devices_loading: bool,
    /// 本地收听历史；与播放会话一样按服务器隔离。
    listening_history: ListeningHistory,
    /// 本地测量的歌曲响度缓存：song_id → (rms_db, peak)。
    loudness_cache: HashMap<String, (f32, f32)>,
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

pub struct NavidromeApp {
    runtime: Runtime,
    api: Option<OpenSubsonicProvider>,
    config: Config,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    audio: AudioHandle,
    next_preload: Option<NextPreload>,
    preload_serial: u64,
    media_session: MediaSession,
    global_shortcuts: GlobalShortcutManager,
    default_cover: Arc<GpuiImage>,
    vinyl_cover: VinylCover,
    state: AppState,
    playlist_ui: PlaylistUi,
    import_ui: ImportUi,
    search_input: Entity<InputState>,
    server_input: Entity<InputState>,
    username_input: Entity<InputState>,
    password_input: Entity<InputState>,
    /// 当前服务器的显示名输入框（可选）。
    name_input: Entity<InputState>,
    playback_slider: Entity<SliderState>,
    volume_slider: Entity<SliderState>,
    muted: bool,
    volume_before_mute: f32,
    volume_save_generation: u64,
    volume_panel_open: bool,
    volume_panel_dragging: bool,
    volume_panel_generation: u64,
    lyrics_scroll_handle: ScrollHandle,
    lyrics_scroll_target: Option<Pixels>,
    content_scroll_handle: ScrollHandle,
    /// 设置卡片内容区的滚动句柄。
    settings_scroll: ScrollHandle,
    /// 每个歌曲列表作用域的 uniform_list 滚动句柄：视口虚拟化 + 按需预取封面。
    song_list_scrolls: RefCell<HashMap<&'static str, UniformListScrollHandle>>,
    /// 待发出的 Toast 通知：错误站点大多拿不到 Window，先入队，
    /// 由 UI 循环统一用 window.push_notification 冲刷。
    pending_notifications: Vec<(NotificationType, SharedString)>,
    cover_slots: Arc<tokio::sync::Semaphore>,
    /// GPUI 会把 `Image` 解码结果保留在全局资产缓存和窗口纹理中；这里只
    /// 记录最近实际渲染过的图片 id，并把超限项排队到 UI 轮询器显式释放。
    cover_image_lru: RefCell<VecDeque<u64>>,
    pending_cover_image_evictions: RefCell<VecDeque<u64>>,
    /// 状态整体重置或同一 id 的源图片被替换时，仍需保留图片句柄到下一次
    /// UI 轮询，才能从 GPUI 全局资产缓存与窗口 atlas 中成对移除。
    pending_cover_asset_releases: RefCell<VecDeque<Arc<GpuiImage>>>,
    title_width_cache: RefCell<HashMap<(u32, FontWeight, SharedString), Pixels>>,
    tray_rx: Receiver<TrayCommand>,
    /// 切歌系统通知发送器（同 ID 替换防堆积）。
    track_notifier: notify::TrackNotifier,
    /// 上一次通知过的曲目 id（曲目变化时才发通知）。
    last_notified_track: Option<String>,
    /// 上一次同步给托盘的暂停状态。
    last_tray_paused: Option<bool>,
    main_hwnd: Option<isize>,
    quitting: Arc<AtomicBool>,
    last_shortcut: String,
    last_shortcut_at: std::time::Instant,
    resume_position: Option<Duration>,
    /// 播放因网络/服务端问题中断后的自动重连计划。
    reconnect: Option<Reconnect>,
    /// 当前曲目已用掉的自动重连次数，以及断点位置；播放确实恢复前进后清零。
    reconnect_attempts: Option<(String, u8, Duration)>,
    last_session_save: std::time::Instant,
    /// 当前本地收听事件及其实际前进时长（seek 跳跃不计入）。
    listening_song_id: Option<String>,
    listening_last_position: Duration,
    listening_accumulated: Duration,
    last_listening_history_save: std::time::Instant,
    active_lyric_index: Option<usize>,
    _subscriptions: Vec<Subscription>,
    mini_mode: bool,
    restore_size: Option<Size<Pixels>>,
    restore_maximized: bool,
    mini_target_size: Option<Size<Pixels>>,
    /// 迷你窗口滚轮调音量的闪光：进度线短暂显示音量，约 1 秒后恢复进度。
    mini_volume_flash: Option<std::time::Instant>,
    mini_volume_flash_generation: u64,
    /// 均衡器 10 段滑杆状态（设置页）。
    eq_sliders: Vec<Entity<SliderState>>,
    /// 均衡器配置保存的防抖代次。
    eq_save_generation: u64,
    /// 每日精选生成代次；换一批、重试和切服务器都会作废旧结果。
    daily_mix_generation: u64,
    /// 艺术家热门歌曲请求代次；重复进入页面或重试时忽略迟到响应。
    artist_top_songs_generation: u64,
    /// 电台请求代次；新请求、手工队列编辑或切服务器都会作废旧结果。
    radio_generation: u64,
    /// `song:<id>` 或 `artist:<id>`，同时用于按钮加载状态和迟到结果校验。
    radio_loading_key: Option<String>,
    /// 自动续播生成代次和加载状态；任何手工队列意图都会使其失效。
    auto_continue_generation: u64,
    auto_continue_loading: bool,
    /// 删除服务器的确认弹窗：（服务器 id， 显示名）。None 表示无弹窗。
    server_remove_prompt: Option<(String, String)>,
}

impl Drop for NavidromeApp {
    fn drop(&mut self) {
        self.finish_listening_event(false);
        // Close the bounded frame channel before Runtime::drop waits for its
        // blocking worker; otherwise a producer could wait on a full queue.
        self.vinyl_cover.cancel_pending();
    }
}

impl NavidromeApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut config = config::load();
        // 界面语言：System 跟随操作系统。渲染处每次 t! 都查询当前
        // locale，因此运行时 set_locale 即时生效，无需重启。
        rust_i18n::set_locale(config.language.resolve());
        // 一次性迁移（里程碑 #17）：把旧版明文密码移入系统凭据库，
        // 迁移成功的条目在配置里只留空引用；凭据库不可用时保持明文，
        // 每轮启动重试，绝不丢密码。
        let migrated = secrets::migrate_config(&mut config);
        if !migrated.is_empty() {
            match config::save(&config) {
                Ok(()) => log::info!(
                    "migrated {} server password(s) into the system credential store",
                    migrated.len()
                ),
                Err(error) => {
                    log::warn!("failed to persist the credential migration: {error:#}")
                }
            }
        }
        // 连接使用解析后的密码：已迁入凭据库的条目从凭据库读取。
        let active_password = config
            .active_profile()
            .map(|profile| profile.resolved_password())
            .unwrap_or_default();
        let api =
            OpenSubsonicProvider::new(&config.server_url, &config.username, &active_password).ok();
        let (tx, rx) = mpsc::channel();
        let runtime = Runtime::new().expect("failed to create Tokio runtime");
        let media_session = MediaSession::new(runtime.handle(), main_window_hwnd(window));
        let global_shortcuts = GlobalShortcutManager::new(&config.global_shortcuts);
        let audio_cache_dir = config::audio_cache_dir(&config);
        let audio = AudioHandle::start(audio_cache_dir, config.output_device.clone())
            .expect("failed to start audio worker");
        audio.set_equalizer(config.equalizer.clone());
        let default_cover = Arc::new(GpuiImage::from_bytes(
            GpuiImageFormat::Png,
            include_bytes!("../assets/default-cover.png").to_vec(),
        ));
        let search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("Search songs, albums, and artists"))
                .clean_on_escape()
        });
        let server_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("http://localhost:4533"))
                .default_value(config.server_url.clone())
        });
        let username_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("Username"))
                .default_value(config.username.clone())
        });
        let password_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("Password"))
                .default_value(active_password)
                .masked(true)
        });
        // 服务器显示名（可选）：为空时界面回退到主机名。
        let name_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(t!("Optional — defaults to the host name"))
                .default_value(
                    config
                        .active_profile()
                        .map(|profile| profile.name.clone())
                        .unwrap_or_default(),
                )
        });
        let playback_slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(100.0)
                .step(0.1)
                .default_value(0.0)
        });
        let volume_slider = cx.new(|_| {
            SliderState::new()
                .min(0.0)
                .max(100.0)
                .step(1.0)
                .default_value(config.volume * 100.0)
        });
        let initial_volume = config.volume;
        let eq_sliders: Vec<Entity<SliderState>> = (0..EqSettings::BANDS.len())
            .map(|band| {
                let gain = config.equalizer.gains_db[band];
                cx.new(move |_| {
                    SliderState::new()
                        .min(-12.0)
                        .max(12.0)
                        .step(1.0)
                        .default_value(gain)
                })
            })
            .collect();

        let mut app = Self {
            runtime,
            api,
            config,
            tx,
            rx,
            audio,
            next_preload: None,
            preload_serial: 0,
            media_session,
            global_shortcuts,
            default_cover,
            vinyl_cover: VinylCover::default(),
            state: AppState::default(),
            playlist_ui: PlaylistUi::default(),
            import_ui: ImportUi::default(),
            search_input: search_input.clone(),
            server_input,
            username_input,
            password_input,
            name_input,
            playback_slider: playback_slider.clone(),
            volume_slider: volume_slider.clone(),
            muted: false,
            volume_before_mute: if initial_volume > 0.001 {
                initial_volume
            } else {
                0.7
            },
            volume_save_generation: 0,
            volume_panel_open: false,
            volume_panel_dragging: false,
            volume_panel_generation: 0,
            lyrics_scroll_handle: ScrollHandle::new(),
            lyrics_scroll_target: None,
            content_scroll_handle: ScrollHandle::new(),
            settings_scroll: ScrollHandle::new(),
            song_list_scrolls: RefCell::new(HashMap::new()),
            pending_notifications: Vec::new(),
            cover_slots: Arc::new(tokio::sync::Semaphore::new(4)),
            cover_image_lru: RefCell::new(VecDeque::new()),
            pending_cover_image_evictions: RefCell::new(VecDeque::new()),
            pending_cover_asset_releases: RefCell::new(VecDeque::new()),
            title_width_cache: RefCell::new(HashMap::new()),
            tray_rx: mpsc::channel().1,
            track_notifier: notify::TrackNotifier::new(),
            last_notified_track: None,
            last_tray_paused: None,
            main_hwnd: None,
            quitting: Arc::new(AtomicBool::new(false)),
            last_shortcut: String::new(),
            last_shortcut_at: std::time::Instant::now(),
            resume_position: None,
            reconnect: None,
            reconnect_attempts: None,
            last_session_save: std::time::Instant::now(),
            listening_song_id: None,
            listening_last_position: Duration::ZERO,
            listening_accumulated: Duration::ZERO,
            last_listening_history_save: std::time::Instant::now(),
            active_lyric_index: None,
            _subscriptions: Vec::new(),
            mini_mode: false,
            restore_size: None,
            restore_maximized: false,
            mini_target_size: None,
            mini_volume_flash: None,
            mini_volume_flash_generation: 0,
            eq_sliders,
            eq_save_generation: 0,
            daily_mix_generation: 0,
            artist_top_songs_generation: 0,
            radio_generation: 0,
            radio_loading_key: None,
            auto_continue_generation: 0,
            auto_continue_loading: false,
            server_remove_prompt: None,
        };
        app.state.playback_modes = app.config.playback_modes;
        // 封面磁盘缓存总量控制：超限时从最旧开始清理（后台线程，不阻塞启动）。
        {
            let prune_config = app.config.clone();
            app.spawn_future(async move {
                config::prune_cover_cache(&prune_config);
            });
        }
        app.state.albums_sort = app.config.albums_sort;
        app.state.artists_sort = app.config.artists_sort;
        app.audio.set_volume(initial_volume);
        // 响度/流派/播放会话都按服务器隔离，统一在切换与启动时加载。
        app.load_active_server_cache();

        // 启动系统托盘（Windows），并拦截"关闭"改为隐藏到托盘。
        let (tray_tx, tray_rx) = mpsc::channel();
        if let Err(error) = tray::start_tray_worker(tray_tx) {
            log::warn!("failed to start tray worker: {error:#}");
        }
        app.tray_rx = tray_rx;
        app.main_hwnd = main_window_hwnd(window);

        // 注册应用级快捷键监听（不依赖焦点；输入框聚焦时由输入框包装层阻止传播）。
        app._subscriptions
            .push(cx.observe_keystrokes(|this, event, window, cx| {
                this.handle_keystroke(&event.keystroke, window, cx);
            }));

        let close_quitting = app.quitting.clone();
        let close_hwnd = app.main_hwnd;
        window.on_window_should_close(cx, move |window, cx| {
            if close_quitting.load(Ordering::Relaxed) {
                // 真正退出前保存最后状态。
                if let Some(Some(root)) = window.root::<NavidromeApp>() {
                    root.update(cx, |app, _cx| app.save_session());
                }
                true
            } else if let Some(hwnd) = close_hwnd {
                hide_main_window(hwnd);
                false
            } else {
                // 平台层关闭（Alt+F4 等）：按设置决定退出还是最小化。
                let close_minimizes = match window.root::<NavidromeApp>() {
                    Some(Some(root)) => root.update(cx, |app, _| app.config.close_minimizes),
                    _ => false,
                };
                if close_minimizes {
                    window.minimize_window();
                    false
                } else {
                    true
                }
            }
        });
        app._subscriptions.push(
            cx.subscribe(&search_input, |this, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.submit_search(cx);
                    cx.notify();
                }
            }),
        );
        app._subscriptions.push(cx.subscribe(
            &playback_slider,
            |this, _, event: &SliderEvent, cx| {
                let SliderEvent::Change(SliderValue::Single(value)) = event else {
                    return;
                };
                this.cancel_next_preload();
                let duration = this.audio.state().duration.unwrap_or_default();
                if !duration.is_zero() {
                    this.seek_playback(duration.mul_f32((*value / 100.0).clamp(0.0, 1.0)));
                }
                cx.notify();
            },
        ));
        app._subscriptions.push(cx.subscribe(
            &volume_slider,
            |this, _, event: &SliderEvent, cx| {
                let SliderEvent::Change(SliderValue::Single(value)) = event else {
                    return;
                };
                this.set_volume(*value / 100.0, cx);
            },
        ));
        let eq_sliders = app.eq_sliders.clone();
        for (band, slider) in eq_sliders.iter().enumerate() {
            app._subscriptions.push(cx.subscribe(
                slider,
                move |this, _, event: &SliderEvent, cx| {
                    let SliderEvent::Change(SliderValue::Single(value)) = event else {
                        return;
                    };
                    this.set_eq_band(band, *value, cx);
                },
            ));
        }
        app._subscriptions
            .push(cx.observe_window_appearance(window, |this, window, cx| {
                if this.config.theme == ThemePreference::System {
                    Theme::sync_system_appearance(Some(window), cx);
                    cx.notify();
                }
            }));

        if app.api.is_some() {
            app.refresh_library();
        } else {
            app.state.status = t!("Open Settings to configure the server").to_string();
        }
        if app.config.username.trim().is_empty() {
            app.state.status = t!("Configure your Navidrome server").to_string();
            app.state.settings_open = true;
        }

        cx.spawn_in(window, async move |this, cx| {
            let mut previous_playback = None;
            let mut playback_dirty = false;
            let mut last_playback_render = std::time::Instant::now();
            let mut was_visible = false;
            loop {
                Timer::after(Duration::from_millis(50)).await;
                if this
                    .update_in(cx, |this, window, cx| {
                        this.sync_gapless_transition();
                        let mut changed = this.poll_tray_commands(window);
                        changed |= this.poll_global_shortcuts(window, cx);
                        changed |= this.poll_media_commands(window, cx);
                        // 单实例激活信号：显示并置前窗口。
                        if single_instance::poll_activation() {
                            if let Some(hwnd) = this.main_hwnd {
                                show_main_window(hwnd);
                            }
                        }
                        this.maybe_save_session();
                        changed |= this.poll_messages(window, cx);
                        this.release_unused_cover_textures(window, cx);
                        // 冲刷排队的 Toast：gpui-component 的通知系统由窗口 Root
                        // 托管（自动消失、可关闭、主题化），需要 Window 句柄。
                        for (kind, message) in this.pending_notifications.drain(..) {
                            window.push_notification(
                                match kind {
                                    NotificationType::Error => Notification::error(message),
                                    NotificationType::Info => Notification::info(message),
                                    NotificationType::Success => Notification::success(message),
                                    NotificationType::Warning => Notification::warning(message),
                                },
                                cx,
                            );
                        }
                        this.handle_playback_end();
                        this.poll_playback_reconnect();
                        this.maybe_preload_next();
                        this.maybe_scrobble_current();
                        this.poll_listening_history();
                        this.maybe_notify_track_started();
                        this.sync_tray_paused();
                        let scrolling = this.lyrics_scroll_target.is_some();
                        this.update_active_lyric();
                        changed |= this.vinyl_cover.poll_ready();
                        let playback = this.audio.state();
                        this.sync_media_session(&playback);
                        // 可见但未聚焦的窗口仍应更新进度、歌词和动画。只有真正
                        // 隐藏/最小化时才暂停重绘，避免切到别的应用后界面“冻住”。
                        let visible = window.is_visible();
                        changed |= this.vinyl_cover.tick(
                            std::time::Instant::now(),
                            visible
                                && this.state.view == View::NowPlaying
                                && !this.state.settings_open
                                && !this.mini_mode
                                && playback.active
                                && !playback.paused,
                        );
                        playback_dirty |= previous_playback.as_ref() != Some(&playback);
                        let transport_changed = previous_playback.as_ref().is_none_or(
                            |last: &crate::audio::PlaybackState| {
                                last.active != playback.active
                                    || last.paused != playback.paused
                                    || last.ended != playback.ended
                                    || last.error != playback.error
                                    || last.duration != playback.duration
                                    || last.transition_id != playback.transition_id
                            },
                        );
                        previous_playback = Some(playback);
                        if !visible {
                            this.vinyl_cover.cancel_pending();
                        }
                        // Keep polling messages/audio while idle, but don't redraw the
                        // whole window for an unchanged or background playback clock.
                        let interval = if this.state.view == View::NowPlaying
                            && !this.state.settings_open
                            && !this.mini_mode
                        {
                            Duration::from_millis(50)
                        } else {
                            Duration::from_millis(250)
                        };
                        if changed
                            || visible
                                && (!was_visible
                                    || transport_changed
                                    || scrolling
                                    || playback_dirty && last_playback_render.elapsed() >= interval)
                        {
                            playback_dirty = false;
                            last_playback_render = std::time::Instant::now();
                            cx.notify();
                        }
                        was_visible = visible;
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        app
    }

    fn spawn_future<F>(&self, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.runtime.spawn(future);
    }

    /// 弹出错误 Toast。错误发生处通常没有 Window（后台消息、设置回写等），
    /// 因此先入队，由 UI 循环用 window.push_notification 发出。
    fn notify_error(&mut self, message: impl Into<SharedString>) {
        self.pending_notifications
            .push((NotificationType::Error, message.into()));
    }

    fn notify_warning(&mut self, message: impl Into<SharedString>) {
        self.pending_notifications
            .push((NotificationType::Warning, message.into()));
    }

    fn notify_success(&mut self, message: impl Into<SharedString>) {
        self.pending_notifications
            .push((NotificationType::Success, message.into()));
    }

    /// 记录 hover 中的列表项 id，供 hover 时浮现的交互元素使用。
    fn set_hovered(&mut self, id: &str, hovering: bool, cx: &mut Context<Self>) {
        if hovering && self.state.hovered_item.as_deref() != Some(id) {
            self.state.hovered_item = Some(id.to_string());
            cx.notify();
        } else if !hovering && self.state.hovered_item.as_deref() == Some(id) {
            self.state.hovered_item = None;
            cx.notify();
        }
    }
}

#[cfg(target_os = "windows")]
fn main_window_hwnd(window: &Window) -> Option<isize> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = HasWindowHandle::window_handle(window) else {
        return None;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return None;
    };
    Some(handle.hwnd.get())
}

#[cfg(not(target_os = "windows"))]
fn main_window_hwnd(_window: &Window) -> Option<isize> {
    None
}

#[cfg(target_os = "windows")]
fn hide_main_window(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    unsafe {
        ShowWindow(hwnd as windows_sys::Win32::Foundation::HWND, SW_HIDE);
    }
}

#[cfg(not(target_os = "windows"))]
fn hide_main_window(_hwnd: isize) {}

#[cfg(target_os = "windows")]
fn show_main_window(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_SHOW};
    unsafe {
        ShowWindow(hwnd as windows_sys::Win32::Foundation::HWND, SW_SHOW);
        SetForegroundWindow(hwnd as windows_sys::Win32::Foundation::HWND);
    }
}

#[cfg(not(target_os = "windows"))]
fn show_main_window(_hwnd: isize) {}

#[cfg(target_os = "windows")]
fn request_window_close(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
    unsafe {
        PostMessageW(hwnd as windows_sys::Win32::Foundation::HWND, WM_CLOSE, 0, 0);
    }
}

#[cfg(target_os = "windows")]
fn set_always_on_top(window: &mut Window, on: bool) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = window.window_handle() else {
        log::warn!("unable to obtain the native window handle");
        return;
    };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return;
    };
    let hwnd = handle.hwnd.get() as windows_sys::Win32::Foundation::HWND;
    let insert_after = if on {
        windows_sys::Win32::UI::WindowsAndMessaging::HWND_TOPMOST
    } else {
        windows_sys::Win32::UI::WindowsAndMessaging::HWND_NOTOPMOST
    };
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SetWindowPos(
            hwnd,
            insert_after,
            0,
            0,
            0,
            0,
            windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
                | windows_sys::Win32::UI::WindowsAndMessaging::SWP_NOSIZE,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn set_always_on_top(_window: &mut Window, on: bool) {
    log::warn!(
        "always-on-top is not implemented on this platform; the mini player will not stay on top (requested={on})"
    );
}

fn technical_info_chip(
    id: &'static str,
    label: String,
    color: Hsla,
    cx: &Context<NavidromeApp>,
) -> gpui_kit::AnyElement {
    div()
        .id(id)
        .h(px(20.0))
        .max_w(px(64.0))
        .px_1p5()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.0))
        .border_1()
        .border_color(color.opacity(0.12))
        .bg(color.opacity(0.045))
        .text_size(px(12.0))
        .text_color(cx.theme().foreground.opacity(0.74))
        .truncate()
        .child(label)
        .into_any_element()
}
