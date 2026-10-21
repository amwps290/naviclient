use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
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
use smol::Timer;
use tokio::runtime::Runtime;

use crate::api::{format_duration, Api};
use crate::assets::{AppIcon, PlayerIcon};
use crate::audio::{
    format_playback, AudioHandle, PlaybackState, StreamFailure, TrackTransition,
    PLAYBACK_RECONNECT_ATTEMPTS,
};
use crate::config;
use crate::media_session::{self, MediaCommand, MediaSession, MediaUpdate};
use crate::models::{
    Album, AlbumSort, Artist, ArtistSort, Config, EqSettings, FavoriteKey, FavoriteKind, Favorites,
    Genre, Lyrics, PlayModes, PlaybackSession, Playlist, RepeatMode, SearchResults, ServerInfo,
    Song, ThemePreference, TranscodingQuality, VolumeNormalization,
};
use crate::msg::{error_message, Msg};
use crate::notify;
use crate::secrets;
use crate::single_instance;
use crate::tray::{self, TrayCommand};
use crate::vinyl::VinylCover;

#[cfg(test)]
mod tests;

mod covers;
mod helpers;
mod library;
mod playlists;
mod queue;
mod views;
use helpers::*;
use playlists::{PlaylistUi, SongMenuContext};

/// 亮色主题次要文字对比度修正（UX 反馈：提示灰在亮色界面看不清）。
///
/// 默认亮色主题 muted.foreground = neutral-500（#737373），在白底上
/// 对比度约 4.7:1；应用的次要文字大量使用 text_xs 小字号并叠在半透明
/// 卡片上，观感发灰发虚。这里把全局 Theme 的默认亮色配置替换为
/// muted 前景加深到 neutral-600（#525252，约 7.4:1）的副本，再重载
/// 当前模式让 colors 立即生效。此后所有 `Theme::change`（设置页切换、
/// System 外观同步）都会从这份配置加载，无需重复调用。深色主题
/// （neutral-400 on neutral-950）不受影响。
pub fn adjust_light_theme_contrast(cx: &mut App) {
    let mode = {
        let theme = Theme::global_mut(cx);
        let mut config = (*theme.light_theme).clone();
        config.colors.muted_foreground = Some("#525252".into());
        theme.light_theme = std::rc::Rc::new(config);
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
    queue_index: Option<usize>,
    now_playing: Option<Song>,
    now_playing_quality: Option<TranscodingQuality>,
    lyrics: Option<Lyrics>,
    lyrics_song_id: Option<String>,
    lyrics_loading: bool,
    lyrics_error: Option<String>,
    covers: HashMap<String, Arc<GpuiImage>>,
    cover_palettes: HashMap<String, (Hsla, Hsla)>,
    requested_covers: HashSet<String>,
    status: String,
    settings_open: bool,
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
            genres: Vec::new(),
            genres_loading: false,
            current_genre: None,
            genre_albums: Vec::new(),
            genre_albums_loading: false,
            genre_albums_page: 0,
            genre_albums_exhausted: false,
            genre_albums_page_failed: false,
            search_filter: SearchFilter::default(),
            collapsed_sections: HashSet::new(),
            current_artist: None,
            artist_albums: Vec::new(),
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
            queue_index: None,
            now_playing: None,
            now_playing_quality: None,
            lyrics: None,
            lyrics_song_id: None,
            lyrics_loading: false,
            lyrics_error: None,
            covers: HashMap::new(),
            cover_palettes: HashMap::new(),
            requested_covers: HashSet::new(),
            status: "Not connected".to_string(),
            settings_open: false,
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
            loudness_cache: HashMap::new(),
        }
    }
}

pub struct NavidromeApp {
    runtime: Runtime,
    api: Option<Api>,
    config: Config,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    audio: AudioHandle,
    next_preload: Option<NextPreload>,
    preload_serial: u64,
    media_session: MediaSession,
    default_cover: Arc<GpuiImage>,
    vinyl_cover: VinylCover,
    state: AppState,
    playlist_ui: PlaylistUi,
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
    last_shortcut_key: String,
    last_shortcut_at: std::time::Instant,
    resume_position: Option<Duration>,
    /// 播放因网络/服务端问题中断后的自动重连计划。
    reconnect: Option<Reconnect>,
    /// 当前曲目已用掉的自动重连次数，以及断点位置；播放确实恢复前进后清零。
    reconnect_attempts: Option<(String, u8, Duration)>,
    last_session_save: std::time::Instant,
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
    /// 删除服务器的确认弹窗：（服务器 id， 显示名）。None 表示无弹窗。
    server_remove_prompt: Option<(String, String)>,
}

impl Drop for NavidromeApp {
    fn drop(&mut self) {
        // Close the bounded frame channel before Runtime::drop waits for its
        // blocking worker; otherwise a producer could wait on a full queue.
        self.vinyl_cover.cancel_pending();
    }
}

impl NavidromeApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut config = config::load();
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
        let api = Api::new(&config.server_url, &config.username, &active_password).ok();
        let (tx, rx) = mpsc::channel();
        let runtime = Runtime::new().expect("failed to create Tokio runtime");
        let media_session = MediaSession::new(runtime.handle());
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
                .placeholder("Search songs, albums, and artists")
                .clean_on_escape()
        });
        let server_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("http://localhost:4533")
                .default_value(config.server_url.clone())
        });
        let username_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Username")
                .default_value(config.username.clone())
        });
        let password_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Password")
                .default_value(active_password)
                .masked(true)
        });
        // 服务器显示名（可选）：为空时界面回退到主机名。
        let name_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Optional — defaults to the host name")
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
            default_cover,
            vinyl_cover: VinylCover::default(),
            state: AppState::default(),
            playlist_ui: PlaylistUi::default(),
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
            title_width_cache: RefCell::new(HashMap::new()),
            tray_rx: mpsc::channel().1,
            track_notifier: notify::TrackNotifier::new(),
            last_notified_track: None,
            last_tray_paused: None,
            main_hwnd: None,
            quitting: Arc::new(AtomicBool::new(false)),
            last_shortcut_key: String::new(),
            last_shortcut_at: std::time::Instant::now(),
            resume_position: None,
            reconnect: None,
            reconnect_attempts: None,
            last_session_save: std::time::Instant::now(),
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
            app.state.status = "Open Settings to configure the server".to_string();
        }
        if app.config.username.trim().is_empty() {
            app.state.status = "Configure your Navidrome server".to_string();
            app.state.settings_open = true;
        }

        cx.spawn_in(window, async move |this, cx| {
            let mut previous_playback = None;
            let mut playback_dirty = false;
            let mut last_playback_render = std::time::Instant::now();
            let mut was_foreground = false;
            loop {
                Timer::after(Duration::from_millis(50)).await;
                if this
                    .update_in(cx, |this, window, cx| {
                        this.sync_gapless_transition();
                        let mut changed = this.poll_tray_commands(window);
                        changed |= this.poll_media_commands(window, cx);
                        // 单实例激活信号：显示并置前窗口。
                        if single_instance::poll_activation() {
                            if let Some(hwnd) = this.main_hwnd {
                                show_main_window(hwnd);
                            }
                        }
                        this.maybe_save_session();
                        changed |= this.poll_messages(window, cx);
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
                        this.maybe_notify_track_started();
                        this.sync_tray_paused();
                        let scrolling = this.lyrics_scroll_target.is_some();
                        this.update_active_lyric();
                        changed |= this.vinyl_cover.poll_ready();
                        let playback = this.audio.state();
                        this.sync_media_session(&playback);
                        let foreground = window.is_visible() && window.is_window_active();
                        changed |= this.vinyl_cover.tick(
                            std::time::Instant::now(),
                            foreground
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
                        if !foreground {
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
                            || foreground
                                && (!was_foreground
                                    || transport_changed
                                    || scrolling
                                    || playback_dirty && last_playback_render.elapsed() >= interval)
                        {
                            playback_dirty = false;
                            last_playback_render = std::time::Instant::now();
                            cx.notify();
                        }
                        was_foreground = foreground;
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

    fn load_lyrics(&mut self, song: &Song) {
        self.active_lyric_index = None;
        if self.state.lyrics_song_id.as_deref() == Some(song.id.as_str())
            && (self.state.lyrics.is_some() || self.state.lyrics_loading)
        {
            return;
        }

        self.state.lyrics_song_id = Some(song.id.clone());
        self.state.lyrics = None;
        self.state.lyrics_error = None;
        let Some(api) = self.api.clone() else {
            self.state.lyrics_loading = false;
            self.state.lyrics_error = Some("Configure a server to load lyrics".to_string());
            return;
        };

        self.state.lyrics_loading = true;
        let song = song.clone();
        let song_id = song.id.clone();
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.lyrics(&song).await.map_err(error_message);
            let _ = tx.send(Msg::Lyrics { song_id, result });
        });
    }

    fn play_queue_index(&mut self, index: usize) {
        self.play_queue_index_at(index, Duration::ZERO);
    }

    fn play_queue_index_at(&mut self, index: usize, start_position: Duration) {
        self.freeze_next_preload();
        let Some(song) = self.state.queue.get(index).cloned() else {
            return;
        };
        let preload_id = self
            .next_preload
            .take()
            .filter(|preload| preload.index == index && start_position.is_zero())
            .map(|preload| preload.id);
        self.resume_position = None;
        log::info!(
            "queue playback selected; index={index} song_id={} title={:?} suffix={:?} declared_bytes={:?}",
            song.id,
            song.title,
            song.suffix,
            song.size
        );
        // Opus 等本地无法解码的音源在 Original 档位自动降级为服务端转码；
        // URL、缓存键、界面音质胶囊、无缝衔接都用同一个有效音质，保持一致。
        let configured = self.config.transcoding_quality;
        let quality = TranscodingQuality::effective_for(
            configured,
            song.suffix.as_deref(),
            song.content_type.as_deref(),
        );
        if quality != configured {
            log::info!(
                "transcode fallback engaged; song_id={} suffix={:?} {} -> {} (the local decoder chain cannot decode this source)",
                song.id,
                song.suffix,
                configured.label(),
                quality.label()
            );
        }
        self.select_playing(index, &song, quality);
        let Some(api) = self.api.clone() else {
            self.notify_error("Configure a server before playing");
            return;
        };
        let max_bit_rate = quality.max_bit_rate();
        log::info!(
            "stream profile selected; song_id={} quality={} max_bit_rate_kbps={max_bit_rate:?}",
            song.id,
            quality.label()
        );
        match api.stream_url(&song.id, max_bit_rate) {
            Ok(url) => {
                let duration = song
                    .duration
                    .and_then(|seconds| u64::try_from(seconds).ok())
                    .map(Duration::from_secs);
                let cache_key = format!(
                    "{}:{}:profile={}",
                    api.base_url(),
                    song.id,
                    quality.cache_profile()
                );
                self.state.now_playing_quality = Some(quality);
                let gain = self.song_gain(&song);
                log::debug!(
                    "volume gain song={} mode={:?} server_track_gain={:?} server_album_gain={:?} measured={:?} factor={gain:.3}",
                    song.id,
                    self.config.volume_normalization,
                    song.replay_gain_track_gain,
                    song.replay_gain_album_gain,
                    self.state.loudness_cache.get(&song.id).copied(),
                );
                self.audio
                    .play(url, cache_key, duration, gain, start_position, preload_id);
                self.report_now_playing(&song, start_position);
            }
            Err(error) => self.notify_error(format!("Stream request failed: {error:#}")),
        }
    }

    /// 通知 Navidrome 当前正在播放的歌曲（每首最多发送一次，不阻塞播放）。
    fn report_now_playing(&mut self, song: &Song, position: Duration) {
        if self.state.now_playing_reported.as_deref() == Some(song.id.as_str()) {
            return;
        }
        self.state.now_playing_reported = Some(song.id.clone());
        let Some(api) = self.api.clone() else { return };
        let song_id = song.id.clone();
        let time_secs = position.as_secs() as u32;
        self.spawn_future(async move {
            if let Err(error) = api.update_now_playing(&song_id, time_secs).await {
                log::warn!("updateNowPlaying failed (non-fatal): {error:#}");
            }
        });
    }

    /// 当前歌曲播放达到有效阈值后，向 Navidrome 发送一次 Scrobble（每首一次）。
    fn maybe_scrobble_current(&mut self) {
        let already_scrobbled = self
            .state
            .scrobbled_song_id
            .as_ref()
            .zip(self.state.now_playing.as_ref())
            .is_some_and(|(scrobbled, playing)| scrobbled == &playing.id);
        if already_scrobbled {
            return;
        }
        let Some(song_id) = self.state.now_playing.as_ref().map(|song| song.id.clone()) else {
            return;
        };
        let playback = self.audio.state();
        if !playback.active || playback.paused {
            return;
        }
        if !should_scrobble(playback.position, playback.duration) {
            return;
        }
        self.state.scrobbled_song_id = Some(song_id.clone());
        let Some(api) = self.api.clone() else { return };
        self.spawn_future(async move {
            if let Err(error) = api.scrobble(&song_id, true).await {
                log::warn!("scrobble failed (non-fatal): {error:#}");
            }
        });
    }

    /// 自然播放结束时，若尚未提交 Scrobble 则提交（完整播放一定有效）。
    fn ensure_scrobble_on_end(&mut self) {
        let Some(song) = self.state.now_playing.clone() else {
            return;
        };
        if self.state.scrobbled_song_id.as_deref() == Some(song.id.as_str()) {
            return;
        }
        self.state.scrobbled_song_id = Some(song.id.clone());
        let Some(api) = self.api.clone() else { return };
        let song_id = song.id.clone();
        self.spawn_future(async move {
            if let Err(error) = api.scrobble(&song_id, true).await {
                log::warn!("scrobble on end failed (non-fatal): {error:#}");
            }
        });
    }

    /// 计算该歌曲的响度补偿增益：优先用服务器 ReplayGain，缺失时用本地测量缓存，都没有则 1.0。
    fn song_gain(&self, song: &Song) -> f32 {
        match self.config.volume_normalization {
            VolumeNormalization::Off => 1.0,
            VolumeNormalization::Track => {
                if song.replay_gain_track_gain.is_some() {
                    replay_gain_factor(
                        VolumeNormalization::Track,
                        song.replay_gain_track_gain,
                        song.replay_gain_track_peak,
                        None,
                        None,
                    )
                } else if let Some(&(rms_db, peak)) = self.state.loudness_cache.get(&song.id) {
                    loudness_gain_factor(rms_db, peak)
                } else {
                    1.0
                }
            }
            VolumeNormalization::Album => {
                if song.replay_gain_album_gain.is_some() {
                    replay_gain_factor(
                        VolumeNormalization::Album,
                        None,
                        None,
                        song.replay_gain_album_gain,
                        song.replay_gain_album_peak,
                    )
                } else if let Some(&(rms_db, peak)) = self.state.loudness_cache.get(&song.id) {
                    loudness_gain_factor(rms_db, peak)
                } else {
                    1.0
                }
            }
        }
    }

    /// 自然结束时把本地测量的歌曲响度写入缓存并持久化，供后续播放做音量补偿。
    fn cache_measured_loudness(&mut self, loudness_db: f32, loudness_peak: f32) {
        if self.config.volume_normalization == VolumeNormalization::Off {
            return;
        }
        let Some(song_id) = self.state.now_playing.as_ref().map(|song| song.id.clone()) else {
            return;
        };
        if loudness_db == 0.0 || !loudness_db.is_finite() {
            return;
        }
        self.state
            .loudness_cache
            .insert(song_id.clone(), (loudness_db, loudness_peak));
        let server_id = self.config.active_server_id().map(str::to_string);
        if let Err(error) = config::save_loudness(server_id.as_deref(), &self.state.loudness_cache)
        {
            log::warn!("loudness cache save failed: {error:#}");
        }
        log::debug!(
            "measured loudness cached song={} rms_db={:.1} peak={:.3}",
            song_id,
            loudness_db,
            loudness_peak,
        );
    }

    fn random_shuffle_next(&self) -> usize {
        let len = self.state.queue.len();
        let current = self.state.queue_index.unwrap_or(0);
        if len <= 1 {
            return current;
        }
        let recent: HashSet<usize> = self
            .state
            .shuffle_history
            .recent((len - 1).min(8))
            .collect();
        let mut candidates: Vec<usize> = (0..len).filter(|index| !recent.contains(index)).collect();
        if candidates.is_empty() {
            candidates = (0..len).filter(|index| *index != current).collect();
        }
        candidates
            .choose(&mut thread_rng())
            .copied()
            .unwrap_or(current)
    }

    fn cancel_next_preload(&mut self) {
        if let Some(event) = self.audio.cancel_preload() {
            self.accept_gapless_transition(event);
        }
        self.next_preload = None;
    }

    fn freeze_next_preload(&mut self) {
        if let Some(event) = self.audio.freeze_preload() {
            self.accept_gapless_transition(event);
        }
    }

    fn sync_gapless_transition(&mut self) {
        if let Some(event) = self.audio.take_transition() {
            self.accept_gapless_transition(event);
        }
    }

    fn select_playing(&mut self, index: usize, song: &Song, quality: TranscodingQuality) {
        self.resume_position = None;
        self.state.queue_index = Some(index);
        self.state.now_playing = Some(song.clone());
        self.state.now_playing_quality = Some(quality);
        self.state.ended_handled = false;
        self.state.now_playing_reported = None;
        self.state.scrobbled_song_id = None;
        self.ensure_cover(song.cover_art.as_deref(), true);
        self.load_lyrics(song);
    }

    fn accept_gapless_transition(&mut self, event: TrackTransition) {
        let Some(next) = self.next_preload.take().filter(|next| next.id == event.id) else {
            log::error!("missing UI reservation for gapless transition {}", event.id);
            return;
        };
        let Some(song) = self.state.queue.get(next.index).cloned() else {
            return;
        };
        self.ensure_scrobble_on_end();
        self.cache_measured_loudness(event.loudness_db, event.loudness_peak);
        if self.state.playback_modes.shuffle
            && self.state.shuffle_history.restore_forward().is_none()
        {
            self.state.shuffle_history.advance(next.index);
        }
        self.select_playing(next.index, &song, next.quality);
        self.report_now_playing(&song, Duration::ZERO);
    }

    fn seek_playback(&mut self, position: Duration) {
        self.cancel_next_preload();
        self.audio.seek(position);
    }

    fn next_queue_index(&self, natural_end: bool) -> Option<usize> {
        next_queue_index(
            self.state.playback_modes,
            self.state.queue_index?,
            self.state.queue.len(),
            natural_end,
            self.state.shuffle_history.forward.last().copied(),
            self.next_preload.map(|preload| preload.index),
            || self.random_shuffle_next(),
        )
    }

    fn maybe_preload_next(&mut self) {
        if self.next_preload.is_some()
            || self.state.ended_handled
            || !should_preload_next(&self.audio.state())
        {
            return;
        }
        let Some(index) = self.next_queue_index(true) else {
            return;
        };
        let Some(song) = self.state.queue.get(index) else {
            return;
        };
        let Some(api) = self.api.as_ref() else { return };
        // 与播放主路径同一套有效音质规则：预加载流必须与正式播放的
        // 档位/缓存键一致，否则无缝衔接会因音质不符而拒绝复用。
        let quality = TranscodingQuality::effective_for(
            self.config.transcoding_quality,
            song.suffix.as_deref(),
            song.content_type.as_deref(),
        );
        let Ok(url) = api.stream_url(&song.id, quality.max_bit_rate()) else {
            return;
        };
        let cache_key = format!(
            "{}:{}:profile={}",
            api.base_url(),
            song.id,
            quality.cache_profile()
        );
        let duration = song
            .duration
            .and_then(|seconds| u64::try_from(seconds).ok())
            .map(Duration::from_secs);
        self.preload_serial = self.preload_serial.wrapping_add(1);
        self.next_preload = Some(NextPreload {
            id: self.preload_serial,
            index,
            quality,
        });
        let gain = self.song_gain(song);
        self.audio
            .preload(self.preload_serial, url, cache_key, duration, gain);
    }

    fn play_next(&mut self, natural_end: bool) {
        self.freeze_next_preload();
        if let Some(next) = self.next_queue_index(natural_end) {
            if self.state.playback_modes.shuffle
                && self.state.shuffle_history.restore_forward().is_none()
            {
                self.state.shuffle_history.advance(next);
            }
            self.play_queue_index(next);
        }
    }

    fn advance_queue(&mut self, forward: bool) {
        self.freeze_next_preload();
        if forward {
            self.play_next(false);
            return;
        }
        let Some(index) = self.state.queue_index else {
            return;
        };
        let len = self.state.queue.len();
        if len == 0 {
            return;
        }

        if self.state.playback_modes.shuffle {
            if let Some(previous) = self.state.shuffle_history.previous() {
                self.play_queue_index(previous);
            }
            return;
        }

        let wraps = self.state.playback_modes.repeat != RepeatMode::Off;
        let Some(next) = advance_index(index, len, forward, wraps) else {
            return;
        };
        self.play_queue_index(next);
    }

    /// 切换随机开关：取消预加载（下一首选择变化）并持久化。
    fn toggle_shuffle(&mut self, cx: &mut Context<Self>) {
        self.cancel_next_preload();
        self.state.playback_modes.shuffle = !self.state.playback_modes.shuffle;
        self.config.playback_modes = self.state.playback_modes;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(format!("Playback mode save failed: {error:#}"));
        }
        self.save_session();
        cx.notify();
    }

    /// 循环模式循环切换：Off → All → One → Off；同样取消预加载并持久化。
    fn cycle_repeat(&mut self, cx: &mut Context<Self>) {
        self.cancel_next_preload();
        self.state.playback_modes.repeat = self.state.playback_modes.repeat.next();
        self.config.playback_modes = self.state.playback_modes;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(format!("Playback mode save failed: {error:#}"));
        }
        self.save_session();
        cx.notify();
    }

    pub fn skip(&mut self, offset: i32) {
        if offset > 0 {
            self.advance_queue(true);
        } else if offset < 0 {
            self.advance_queue(false);
        }
    }

    /// 相对当前进度快进/快退（秒），不越过歌曲起点与终点。
    fn seek_by(&mut self, delta_secs: i64) {
        self.cancel_next_preload();
        let playback = self.audio.state();
        let target = seek_target(playback.position, playback.duration, delta_secs);
        if target != playback.position {
            self.seek_playback(target);
        }
    }

    /// 处理全局键盘快捷键；输入框聚焦时不会触发（由输入框包装层阻止传播）。
    fn handle_keystroke(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx)
            || window.has_focused_input(cx)
            || self
                .playlist_ui
                .menu_focus
                .as_ref()
                .is_some_and(|focus| focus.contains_focused(window, cx))
        {
            return;
        }
        let shortcut = match_shortcut(&keystroke.key, keystroke.modifiers);
        if shortcut == Shortcut::None {
            return;
        }
        // 忽略按住重复触发的同一按键（250ms 内同键视为重复）。
        let now = std::time::Instant::now();
        let repeated = self.last_shortcut_key == keystroke.key
            && now.duration_since(self.last_shortcut_at) < Duration::from_millis(250);
        self.last_shortcut_key = keystroke.key.clone();
        self.last_shortcut_at = now;
        // 选择移动豁免重复保护：按住 Shift+↑/↓ 可连续移动。
        if repeated && !matches!(shortcut, Shortcut::SelectPrevious | Shortcut::SelectNext) {
            return;
        }
        match shortcut {
            Shortcut::SelectPrevious => {
                self.move_song_selection(-1, cx);
            }
            Shortcut::SelectNext => {
                self.move_song_selection(1, cx);
            }
            Shortcut::SelectAll => {
                self.select_all_songs(cx);
            }
            Shortcut::PlaySelected => {
                self.play_selected_song(cx);
            }
            Shortcut::TogglePlayback => {
                self.toggle_playback();
                cx.notify();
            }
            Shortcut::Previous => {
                self.skip(-1);
                cx.notify();
            }
            Shortcut::Next => {
                self.skip(1);
                cx.notify();
            }
            Shortcut::SeekBack => {
                self.seek_by(-5);
                cx.notify();
            }
            Shortcut::SeekForward => {
                self.seek_by(5);
                cx.notify();
            }
            Shortcut::VolumeUp => self.adjust_volume(0.05, window, cx),
            Shortcut::VolumeDown => self.adjust_volume(-0.05, window, cx),
            Shortcut::ToggleMute => self.toggle_mute(window, cx),
            Shortcut::ToggleNowPlaying => {
                if self.state.view == View::NowPlaying {
                    self.leave_now_playing();
                } else {
                    self.open_now_playing();
                }
                cx.notify();
            }
            Shortcut::ToggleQueue => {
                if self.state.view == View::Queue {
                    self.state.view = self.state.view_before_now_playing;
                } else {
                    if self.state.view != View::NowPlaying {
                        self.state.view_before_now_playing = self.state.view;
                    }
                    self.state.view = View::Queue;
                }
                cx.notify();
            }
            Shortcut::CloseOverlays => {
                if self.state.song_list_multi.is_some() {
                    // 多选活跃时 Esc 只清空多选，不连带关闭页面/设置。
                    self.clear_song_multi();
                } else if self.state.settings_open {
                    self.state.settings_open = false;
                } else if self.state.view == View::NowPlaying {
                    self.leave_now_playing();
                } else if matches!(
                    self.state.view,
                    View::ArtistDetail | View::AlbumDetail | View::PlaylistDetail
                ) {
                    self.state.view = View::Home;
                }
                cx.notify();
            }
            Shortcut::FocusSearch => {
                // 先切到搜索页，再聚焦搜索框。
                self.state.settings_open = false;
                self.state.view = View::Search;
                cx.notify();
                let mut async_cx = window.to_async(cx);
                let _ = self
                    .search_input
                    .update_in(&mut async_cx, |input, window, cx| input.focus(window, cx));
            }
            Shortcut::None => {}
        }
    }

    /// 手动重试当前由：从出错位置（有位置时）重新拉流，并重置自动重连计数。
    pub(crate) fn retry_current_track(&mut self) {
        let Some(index) = self.state.queue_index else {
            return;
        };
        let playback = self.audio.state();
        let position = if playback.error.is_some() {
            self.resume_position.unwrap_or(playback.position)
        } else {
            playback.position
        };
        log::info!(
            "manual playback retry requested; index={index} position_ms={}",
            position.as_millis()
        );
        self.reconnect = None;
        self.reconnect_attempts = None;
        self.state.ended_handled = false;
        self.play_queue_index_at(index, position);
    }

    pub fn toggle_playback(&mut self) {
        self.sync_gapless_transition();
        let playback = self.audio.state();
        if playback.active {
            if playback.paused {
                self.audio.resume()
            } else {
                self.audio.pause()
            }
        } else if let Some(index) = self.state.queue_index {
            let position = self.resume_position.take().unwrap_or_default();
            self.play_queue_index_at(index, position);
        }
    }

    #[allow(dead_code)]
    pub fn stop_playback(&mut self) {
        self.cancel_next_preload();
        self.audio.stop();
        self.resume_position = None;
        self.state.now_playing = None;
        self.state.now_playing_quality = None;
        self.state.lyrics = None;
        self.state.lyrics_song_id = None;
        self.state.lyrics_loading = false;
        self.state.lyrics_error = None;
        self.state.ended_handled = true;
    }

    fn media_capabilities(&self) -> (bool, bool, bool) {
        let Some(index) = self
            .state
            .queue_index
            .filter(|i| *i < self.state.queue.len())
        else {
            return (false, false, false);
        };
        let can_play = self.api.is_some();
        let wraps = self.state.playback_modes.repeat != RepeatMode::Off;
        let (next, previous) = if self.state.playback_modes.shuffle {
            (true, self.state.shuffle_history.played.len() > 1)
        } else {
            (
                advance_index(index, self.state.queue.len(), true, wraps).is_some(),
                advance_index(index, self.state.queue.len(), false, wraps).is_some(),
            )
        };
        (can_play, can_play && next, can_play && previous)
    }

    fn sync_media_session(&mut self, playback: &crate::audio::PlaybackState) {
        let (can_play, can_next, can_previous) = self.media_capabilities();
        let song = self.state.now_playing.as_ref();
        let cover = song
            .and_then(|song| song.cover_art.as_ref())
            .and_then(|id| self.state.covers.get(id));
        self.media_session.update(MediaUpdate {
            server: self.api.as_ref().map_or("", |api| api.base_url()),
            song,
            cover,
            playback,
            position: self.resume_position.unwrap_or(playback.position),
            volume: effective_volume(self.config.volume, self.muted),
            can_play,
            can_next,
            can_previous,
        });
    }

    fn media_seek(&mut self, position: Duration) {
        if self.audio.state().active {
            self.seek_playback(position);
        } else if self.state.now_playing.is_some() {
            // Seeking a restored, paused session must not start audio playback.
            self.resume_position = Some(position);
            self.media_session.restored_position_changed();
        }
    }

    fn poll_media_commands(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        // Maintain intent across a batch, even before the audio worker consumes
        // it. Repeated Play/Pause commands are idempotent, unlike PlayPause.
        let playback = self.audio.state();
        let mut active = playback.active;
        let mut paused = playback.paused;
        for _ in 0..32 {
            let Some(command) = self.media_session.next_command() else {
                break;
            };
            changed = true;
            match command {
                MediaCommand::Play | MediaCommand::Pause | MediaCommand::PlayPause => {
                    let play = match command {
                        MediaCommand::Play => true,
                        MediaCommand::Pause => false,
                        _ => !active || paused,
                    };
                    if play && self.media_capabilities().0 {
                        if active {
                            if paused {
                                self.audio.resume();
                            }
                        } else if let Some(index) = self.state.queue_index {
                            let position = self.resume_position.take().unwrap_or_default();
                            self.play_queue_index_at(index, position);
                            active = true;
                        }
                        paused = false;
                    } else if !play && active && !paused {
                        self.audio.pause();
                        paused = true;
                    }
                }
                MediaCommand::Stop => {
                    self.stop_playback();
                    active = false;
                    paused = false;
                }
                MediaCommand::Next | MediaCommand::Previous => {
                    let next = matches!(command, MediaCommand::Next);
                    let (_, can_next, can_previous) = self.media_capabilities();
                    if (next && can_next) || (!next && can_previous) {
                        self.skip(if next { 1 } else { -1 });
                        active = true;
                        paused = false;
                    }
                }
                MediaCommand::Seek(offset) => {
                    self.cancel_next_preload();
                    if self.state.now_playing.is_some() {
                        let playback = self.audio.state();
                        let position = self.resume_position.unwrap_or(playback.position);
                        let duration = playback.duration.or_else(|| {
                            self.state
                                .now_playing
                                .as_ref()
                                .and_then(|song| song.duration)
                                .map(|v| Duration::from_secs(v.max(0) as u64))
                        });
                        if offset > 0
                            && duration.is_some_and(|limit| {
                                position.saturating_add(Duration::from_micros(offset as u64))
                                    > limit
                            })
                        {
                            self.skip(1);
                        } else {
                            self.media_seek(media_session::seek_offset(position, offset, duration));
                        }
                    }
                }
                MediaCommand::SetPosition { track_id, position } => {
                    self.cancel_next_preload();
                    if let Some(song) = self.state.now_playing.as_ref() {
                        let server = self.api.as_ref().map_or("", |api| api.base_url());
                        let duration = self.audio.state().duration.or_else(|| {
                            song.duration.map(|v| Duration::from_secs(v.max(0) as u64))
                        });
                        if media_session::track_id(server, &song.id) == track_id
                            && duration.is_none_or(|limit| position <= limit)
                        {
                            self.media_seek(position);
                        }
                    }
                }
                MediaCommand::Volume(volume) => {
                    self.volume_slider.update(cx, |slider, cx| {
                        slider.set_value(volume * 100.0, window, cx)
                    });
                    self.set_volume(volume, cx);
                }
                MediaCommand::Raise => window.activate_window(),
                MediaCommand::Quit => {
                    self.save_session();
                    self.quitting.store(true, Ordering::Relaxed);
                    cx.quit();
                    break;
                }
            }
        }
        changed
    }

    fn enter_mini_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.settings_open = false;
        self.mini_mode = true;
        self.restore_size = Some(window.viewport_size());
        self.restore_maximized = window.is_maximized();
        // The window shadow/border (Linux client-side decorations) shrinks the
        // drawable area by its padding, so request a larger outer window to keep
        // the mini player content at the intended size on every platform.
        let paddings = window_paddings(window);
        let mini_size = size(
            MINI_WINDOW_WIDTH + paddings.left + paddings.right,
            MINI_WINDOW_HEIGHT + paddings.top + paddings.bottom,
        );
        // Lock the mini window to a fixed size: remember the target and snap back
        // to it in render() if the user drags a border to resize it.
        self.mini_target_size = Some(mini_size);
        window.resize(mini_size);
        set_always_on_top(window, true);
        cx.notify();
    }

    fn exit_mini_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mini_mode = false;
        self.mini_target_size = None;
        window.resize(
            self.restore_size
                .take()
                .unwrap_or_else(|| size(px(1280.0), px(820.0))),
        );
        if self.restore_maximized && !window.is_maximized() {
            window.zoom_window();
        }
        self.restore_maximized = false;
        set_always_on_top(window, false);
        cx.notify();
    }

    fn toggle_mini_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mini_mode {
            self.exit_mini_mode(window, cx);
        } else {
            self.enter_mini_mode(window, cx);
        }
    }

    fn set_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        let volume = normalize_volume(volume);
        self.muted = false;
        self.config.volume = volume;
        if volume > 0.001 {
            self.volume_before_mute = volume;
        }
        self.audio.set_volume(volume);
        self.schedule_volume_save(cx);
        cx.notify();
    }

    fn adjust_volume(&mut self, delta: f32, window: &mut Window, cx: &mut Context<Self>) {
        let volume = normalize_volume(self.config.volume + delta);
        self.volume_slider.update(cx, |slider, cx| {
            slider.set_value(volume * 100.0, window, cx);
        });
        self.set_volume(volume, cx);
    }

    fn toggle_mute(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.muted || self.config.volume <= 0.001 {
            let volume = restored_volume(self.volume_before_mute);
            self.muted = false;
            self.config.volume = volume;
            self.audio.set_volume(volume);
            self.volume_slider.update(cx, |slider, cx| {
                slider.set_value(volume * 100.0, window, cx);
            });
            self.schedule_volume_save(cx);
        } else {
            self.volume_before_mute = self.config.volume;
            self.muted = true;
            self.audio.set_volume(0.0);
        }
        cx.notify();
    }

    fn schedule_volume_save(&mut self, cx: &mut Context<Self>) {
        self.volume_save_generation = self.volume_save_generation.wrapping_add(1);
        let generation = self.volume_save_generation;
        cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_millis(500)).await;
            this.update(cx, |this, cx| {
                if this.volume_save_generation != generation {
                    return;
                }
                if let Err(error) = config::save(&this.config) {
                    this.notify_error(format!("Volume save failed: {error:#}"));
                }
                cx.notify();
            })?;
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    /// 迷你窗口滚轮调音量时在进度线位置闪光显示音量；
    /// 计时结束后补一次重绘，避免暂停（无周期重绘）时闪光停留。
    fn show_mini_volume_flash(&mut self, cx: &mut Context<Self>) {
        self.mini_volume_flash = Some(std::time::Instant::now());
        self.mini_volume_flash_generation = self.mini_volume_flash_generation.wrapping_add(1);
        let generation = self.mini_volume_flash_generation;
        cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_millis(1100)).await;
            this.update(cx, |this, cx| {
                if this.mini_volume_flash_generation == generation {
                    this.mini_volume_flash = None;
                    cx.notify();
                }
            })?;
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    fn show_volume_panel(&mut self, cx: &mut Context<Self>) {
        self.volume_panel_generation = self.volume_panel_generation.wrapping_add(1);
        self.volume_panel_open = true;
        cx.notify();
    }

    fn schedule_volume_panel_close(&mut self, cx: &mut Context<Self>) {
        self.volume_panel_generation = self.volume_panel_generation.wrapping_add(1);
        let generation = self.volume_panel_generation;
        cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_millis(220)).await;
            this.update(cx, |this, cx| {
                if this.volume_panel_generation == generation && !this.volume_panel_dragging {
                    this.volume_panel_open = false;
                    cx.notify();
                }
            })?;
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    fn begin_volume_panel_drag(&mut self, cx: &mut Context<Self>) {
        self.volume_panel_dragging = true;
        self.show_volume_panel(cx);
    }

    fn end_volume_panel_drag(&mut self, keep_open: bool, cx: &mut Context<Self>) {
        self.volume_panel_dragging = false;
        if keep_open {
            self.show_volume_panel(cx);
        } else {
            self.schedule_volume_panel_close(cx);
        }
    }

    fn open_now_playing(&mut self) {
        if self.state.view != View::NowPlaying {
            self.state.view_before_now_playing = self.state.view;
        }
        self.state.settings_open = false;
        self.state.view = View::NowPlaying;
        self.active_lyric_index = None;
        self.lyrics_scroll_target = None;
    }

    fn leave_now_playing(&mut self) {
        self.state.view = self.state.view_before_now_playing;
        self.active_lyric_index = None;
        self.lyrics_scroll_target = None;
    }

    /// 切歌系统通知开关。
    fn set_show_track_notifications(&mut self, show: bool, cx: &mut Context<Self>) {
        self.config.show_track_notifications = show;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(format!("Notification setting save failed: {error:#}"));
        }
        cx.notify();
    }

    /// 关闭按钮最小化开关。
    fn set_close_minimizes(&mut self, minimize: bool, cx: &mut Context<Self>) {
        self.config.close_minimizes = minimize;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(format!("Close behavior save failed: {error:#}"));
        }
        cx.notify();
    }

    /// 标题栏 X 按钮（Linux CSD）的关闭处理：按设置最小化或退出。
    /// on_close_window 仅 Linux 生效，其他平台维持默认行为。
    fn close_button_handler(
        cx: &Context<Self>,
    ) -> impl Fn(&gpui_kit::ClickEvent, &mut Window, &mut App) + 'static {
        let app = cx.entity();
        move |_, window, cx| {
            app.update(cx, |this, _| {
                if this.config.close_minimizes {
                    window.minimize_window();
                } else {
                    window.remove_window();
                }
            });
        }
    }

    /// 切歌系统通知：正在播放曲目变化且音频已接通时发送一次。
    /// 音频未接通（切换瞬间）时先不记录，等下一次轮询再发。
    fn maybe_notify_track_started(&mut self) {
        if !self.config.show_track_notifications {
            return;
        }
        let Some(song) = self.state.now_playing.clone() else {
            self.last_notified_track = None;
            return;
        };
        if self.last_notified_track.as_deref() == Some(song.id.as_str()) {
            return;
        }
        if !self.audio.state().active {
            return;
        }
        self.last_notified_track = Some(song.id.clone());
        let cover = song
            .cover_art
            .as_deref()
            .map(|id| config::cover_cache_path(&self.config, self.config.active_server_id(), id));
        self.track_notifier.track_changed(
            &song.title,
            &notify::notification_body(&song.artist, &song.album),
            cover.as_deref(),
        );
    }

    /// 播放/暂停状态变化时同步托盘菜单文案（Play/Pause）。
    fn sync_tray_paused(&mut self) {
        let paused = self.audio.state().paused;
        if self.last_tray_paused != Some(paused) {
            self.last_tray_paused = Some(paused);
            tray::set_tray_paused(paused);
        }
    }

    /// 加载当前服务器作用域下的响度缓存、流派缓存与播放会话。
    /// 启动与切换服务器时都走这里；切换时调用方已清空 `state`。
    fn load_active_server_cache(&mut self) {
        let server_id = self.config.active_server_id().map(str::to_string);
        self.state.loudness_cache = config::load_loudness(server_id.as_deref());
        // 流派是服务端重查询（全库聚合）：先用磁盘缓存即时展示，后台刷新后覆盖。
        self.state.genres = config::load_genres_cache(server_id.as_deref()).unwrap_or_default();

        // 恢复该服务器的播放会话（不自动播放，点播放时从保存位置继续）。
        let Some(session) =
            config::load_session(server_id.as_deref(), self.config.server_url.trim())
        else {
            return;
        };
        if session.queue.is_empty() {
            return;
        }
        // 会话里记录的地址必须与当前服务器一致，避免拿错队列。
        let server_matches = self
            .api
            .as_ref()
            .map(|api| api.base_url() == session.server_url)
            .unwrap_or(false);
        if !server_matches {
            return;
        }
        let index = session
            .queue_index
            .filter(|index| *index < session.queue.len());
        let restored_index = index;
        self.state.queue = session.queue;
        self.state.queue_index = index;
        self.state.playback_modes = session.playback_modes;
        self.config.playback_modes = session.playback_modes;
        self.state.shuffle_history =
            ShuffleHistory::restore(session.shuffle_played, session.shuffle_forward);
        if let Some(index) = restored_index {
            if let Some(song) = self.state.queue.get(index).cloned() {
                self.state.now_playing = Some(song.clone());
                self.resume_position =
                    Some(Duration::from_secs_f64(session.position_secs.max(0.0)));
                self.ensure_cover(song.cover_art.as_deref(), true);
            }
        }
        log::info!(
            "restored playback session; server={:?} queue={} index={restored_index:?} position={:.1}s",
            self.config.active_server_id(),
            self.state.queue.len(),
            session.position_secs
        );
    }

    /// 把设置页的四个输入框同步为当前服务器（切换后必须调用，否则
    /// 用户看到并保存的是上一台服务器的账号）。密码同样解析后展示，
    /// 已迁入凭据库的条目回显真实密码，用户改密码时无需重新输入。
    fn sync_server_inputs(&self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.config.server_url.clone();
        let username = self.config.username.clone();
        let password = self
            .config
            .active_profile()
            .map(|profile| profile.resolved_password())
            .unwrap_or_default();
        let display_name = self
            .config
            .active_profile()
            .map(|profile| profile.name.clone())
            .unwrap_or_default();
        self.name_input
            .update(cx, |input, cx| input.set_value(display_name, window, cx));
        self.server_input
            .update(cx, |input, cx| input.set_value(url, window, cx));
        self.username_input
            .update(cx, |input, cx| input.set_value(username, window, cx));
        self.password_input
            .update(cx, |input, cx| input.set_value(password, window, cx));
    }

    /// 断开当前服务器并重建连接与视图状态（切服务器、保存设置共用）。
    fn reconnect_active_server(&mut self) {
        self.cancel_next_preload();
        self.audio.stop();
        self.state = AppState::default();
        self.playlist_ui.reset();
        self.resume_position = None;
        self.reconnect = None;
        self.reconnect_attempts = None;
        // 上一台服务器的曲目不应继续出现在“正在播放”与通知里。
        self.last_notified_track = None;
        self.state.playback_modes = self.config.playback_modes;
        self.state.albums_sort = self.config.albums_sort;
        self.state.artists_sort = self.config.artists_sort;
        // 空白条目（刚新增还没填）不是错误：只清空连接，等用户保存地址。
        let configured = self
            .config
            .active_profile()
            .is_some_and(|profile| profile.is_configured());
        if !configured {
            self.api = None;
            self.state.status = "Enter the server address and account".to_string();
            self.state.settings_open = true;
            return;
        }
        // 已迁入系统凭据库的条目用解析后的密码连接。
        let password = self
            .config
            .active_profile()
            .map(|profile| profile.resolved_password())
            .unwrap_or_default();
        match Api::new(&self.config.server_url, &self.config.username, &password) {
            Ok(api) => {
                self.api = Some(api);
                self.load_active_server_cache();
                self.refresh_library();
            }
            Err(error) => {
                self.api = None;
                self.state.status = "Invalid server URL".to_string();
                self.notify_error(format!("{error:#}"));
                self.state.settings_open = true;
            }
        }
    }

    /// 切换到另一台已保存的服务器。切换前先把当前服务器的作用域数据落盘。
    fn switch_server(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.config.active_server.as_deref() == Some(id) {
            return;
        }
        if !self.config.servers.iter().any(|profile| profile.id == id) {
            return;
        }
        // 队列属于当前服务器：先保存，再断开。
        self.save_session();
        if let Err(error) =
            config::save_loudness(self.config.active_server_id(), &self.state.loudness_cache)
        {
            log::warn!("failed to save loudness cache: {error:#}");
        }
        if !self.config.switch_to(id) {
            return;
        }
        if let Err(error) = config::save(&self.config) {
            self.notify_error(format!("Config save failed: {error:#}"));
        }
        log::info!(
            "switched server; id={id} url={} user={}",
            self.config.server_url,
            self.config.username
        );
        self.state.status = format!("Switched to {}", self.active_server_label());
        self.reconnect_active_server();
        self.sync_server_inputs(window, cx);
    }

    /// 新增一台空白服务器并立即切过去，方便直接填地址账号。
    fn add_server(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_session();
        // 切走前把当前服务器的作用域数据落盘（与 switch_server 同等义务）。
        if let Err(error) =
            config::save_loudness(self.config.active_server_id(), &self.state.loudness_cache)
        {
            log::warn!("failed to save loudness cache: {error:#}");
        }
        let id = self.config.add_server();
        if let Err(error) = config::save(&self.config) {
            self.notify_error(format!("Config save failed: {error:#}"));
        }
        log::info!("added server entry; id={id}");
        self.reconnect_active_server();
        self.sync_server_inputs(window, cx);
        self.state.status = "Enter the address and account for the new server".to_string();
    }

    /// 删除一台服务器；正在使用的条目被删除时自动切到相邻条目。
    /// clean_data = true 时一并清理这台服务器的作用域缓存（播放会话/
    /// 响度/流派）与系统凭据库条目；凭据删除不可恢复，重新添加同一台
    /// 服务器需要重输密码，因此默认保留。
    fn remove_server(
        &mut self,
        id: &str,
        clean_data: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let removing_active = self.config.active_server.as_deref() == Some(id);
        let Some(profile) = self
            .config
            .servers
            .iter()
            .find(|profile| profile.id == id)
            .cloned()
        else {
            return;
        };
        if removing_active {
            self.save_session();
            // 与切换服务器同等的落盘义务：响度缓存也属于当前服务器。
            if let Err(error) =
                config::save_loudness(self.config.active_server_id(), &self.state.loudness_cache)
            {
                log::warn!("failed to save loudness cache: {error:#}");
            }
        }
        if !self.config.remove_server(id) {
            self.notify_error("Keep at least one server configured");
            return;
        }
        if clean_data {
            if profile.password_in_keyring {
                secrets::delete_password(id);
            }
            let removed = config::remove_server_cache(id);
            log::info!("removed {removed} scoped cache file(s) of server {id}");
        }
        if let Err(error) = config::save(&self.config) {
            self.notify_error(format!("Config save failed: {error:#}"));
        }
        log::info!("removed server entry; id={id}");
        if removing_active {
            self.reconnect_active_server();
            self.sync_server_inputs(window, cx);
        }
        self.state.status = format!("Removed {}", profile.label());
    }

    /// 删除服务器前的确认弹窗（可选连同清理这台服务器的本地数据）。
    pub(super) fn confirm_remove_server(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(label) = self
            .config
            .servers
            .iter()
            .find(|profile| profile.id == id)
            .map(|profile| profile.label())
        else {
            return;
        };
        if self.server_remove_prompt.is_some() {
            return;
        }
        self.server_remove_prompt = Some((id.to_string(), label));
        self.show_remove_server_dialog(window, cx);
    }

    /// 弹窗确认后的实际删除；clean_data 的语义与 remove_server 一致。
    /// 弹窗的关闭由调用方（按钮/框架）负责，这里只执行删除。
    pub(super) fn submit_remove_server(
        &mut self,
        clean_data: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((id, _)) = self.server_remove_prompt.take() else {
            return;
        };
        self.remove_server(&id, clean_data, window, cx);
        cx.notify();
    }

    /// 关闭弹窗（取消 / Esc / 点击遮罩）：只清状态，不删除。
    pub(super) fn cancel_remove_server(&mut self, cx: &mut Context<Self>) {
        self.server_remove_prompt = None;
        cx.notify();
    }

    fn show_remove_server_dialog(&self, window: &mut Window, cx: &mut Context<Self>) {
        if !window.has_active_dialog(cx) {
            let app = cx.entity().downgrade();
            // 与歌单弹窗同因：先等菜单层收尾，再安装弹窗的焦点域。
            window.defer(cx, move |window, cx| {
                window.open_dialog(cx, move |dialog, _, cx| {
                    app.update(cx, |this, cx| this.render_remove_server_dialog(dialog, cx))
                        .expect("app owns dialog")
                });
            });
        }
        cx.notify();
    }

    /// 当前服务器的显示名（侧边栏与设置页顶部共用）。
    fn active_server_label(&self) -> String {
        self.config
            .active_profile()
            .map(|profile| profile.label())
            .unwrap_or_else(|| "No server".to_string())
    }

    fn save_settings(&mut self, cx: &Context<Self>) {
        let url = self.server_input.read(cx).value().trim().to_string();
        let username = self.username_input.read(cx).value().trim().to_string();
        let password = self.password_input.read(cx).value().to_string();
        let display_name = self.name_input.read(cx).value().trim().to_string();
        // 写回当前服务器条目，而不是另外造一份配置：多服务器列表是唯一真源。
        let active_id = self.config.store_active(&url, &username, &password);
        if let Some(profile) = self
            .config
            .servers
            .iter_mut()
            .find(|profile| profile.id == active_id)
        {
            profile.name = display_name;
            // 密码优先写入系统凭据库（里程碑 #17）：成功后配置里只留空
            // 引用，明文不再落盘；凭据库不可用时保留明文并落下标记。
            if !password.is_empty() {
                match secrets::store_password(&active_id, &username, &password) {
                    Ok(()) => {
                        profile.password.clear();
                        profile.password_in_keyring = true;
                        log::info!(
                            "stored the password of server {active_id} in the system credential store"
                        );
                    }
                    Err(error) => {
                        profile.password_in_keyring = false;
                        log::warn!(
                            "system credential store unavailable; keeping the password in the config file: {error:#}"
                        );
                    }
                }
            } else {
                // 用户主动清空密码：同步删掉凭据库条目，避免遗留旧密码。
                if profile.password_in_keyring {
                    secrets::delete_password(&active_id);
                }
                profile.password_in_keyring = false;
            }
        }
        // 同步镜像字段（server_url/username/password 始终与激活条目一致）。
        // 直接在 self.config 上变更，不再重建整份 Config——手动罗列字段的
        // 重建在新字段加入时极易漏拷而静默丢失设置。
        self.config.sync_active_profile();
        if let Err(error) = config::save(&self.config) {
            self.state.status = format!("Config save failed: {error}");
        }
        self.reconnect_active_server();
    }

    fn apply_cache_directory(&mut self, cache_dir: Option<PathBuf>) {
        let effective_dir = cache_dir
            .clone()
            .unwrap_or_else(config::default_audio_cache_dir);
        if let Err(error) = fs::create_dir_all(&effective_dir) {
            self.notify_error(format!(
                "Unable to use cache directory {}: {error}",
                effective_dir.display()
            ));
            return;
        }

        self.config.cache_dir = cache_dir;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(format!("Cache directory save failed: {error:#}"));
            return;
        }
        self.cancel_next_preload();
        self.audio.set_cache_directory(effective_dir.clone());
        self.state.status = format!("Audio cache: {}", effective_dir.display());
    }

    fn choose_cache_directory(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Select audio cache folder".into()),
        });
        cx.spawn(async move |this, cx| {
            let selected = receiver
                .await
                .ok()
                .and_then(Result::ok)
                .flatten()
                .and_then(|paths| paths.into_iter().next());
            if let Some(path) = selected {
                this.update(cx, |this, cx| {
                    this.apply_cache_directory(Some(path));
                    cx.notify();
                })?;
            }
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    fn set_transcoding_quality(&mut self, quality: TranscodingQuality, cx: &mut Context<Self>) {
        self.cancel_next_preload();
        self.config.transcoding_quality = quality;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(format!("Playback quality save failed: {error:#}"));
        } else {
            log::info!("transcoding quality changed; quality={}", quality.label());
        }
        cx.notify();
    }

    fn set_volume_normalization(&mut self, mode: VolumeNormalization, cx: &mut Context<Self>) {
        self.cancel_next_preload();
        self.config.volume_normalization = mode;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(format!("Volume normalization save failed: {error:#}"));
        }
        cx.notify();
    }

    /// 歌词翻译行显示开关；服务器无多语言变体时该开关无效果。
    fn set_show_lyrics_translation(&mut self, show: bool, cx: &mut Context<Self>) {
        self.config.show_lyrics_translation = show;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(format!("Lyrics translation save failed: {error:#}"));
        }
        cx.notify();
    }

    /// 枚举输出设备（后台线程，避免 cpal 枚举阻塞 UI）。
    fn refresh_output_devices(&mut self, cx: &mut Context<Self>) {
        if self.state.output_devices_loading {
            return;
        }
        self.state.output_devices_loading = true;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = crate::audio::list_output_devices().map_err(error_message);
            let _ = tx.send(Msg::OutputDevices(result));
        });
        cx.notify();
    }

    /// 单段均衡增益变化（滑杆拖动）：更新配置、推送音频线程、防抖保存。
    /// 任何拖动都会开启均衡器（与预设一致的心智模型）。
    fn set_eq_band(&mut self, band: usize, gain_db: f32, cx: &mut Context<Self>) {
        let Some(current) = self.config.equalizer.gains_db.get_mut(band) else {
            return;
        };
        if (*current - gain_db).abs() < f32::EPSILON && self.config.equalizer.enabled {
            return;
        }
        *current = gain_db;
        self.config.equalizer.clamp_gains();
        self.config.equalizer.enabled = true;
        self.push_equalizer_to_audio();
        self.schedule_eq_save(cx);
        cx.notify();
    }

    /// 均衡器开关。
    fn set_eq_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.config.equalizer.enabled == enabled {
            return;
        }
        self.config.equalizer.enabled = enabled;
        self.push_equalizer_to_audio();
        self.schedule_eq_save(cx);
        cx.notify();
    }

    /// 应用预设（不含滑杆显示同步——由调用方在可用的上下文里完成）：
    /// 更新配置、推送音频线程并开启均衡器。
    fn set_eq_preset_values(&mut self, gains: [f32; 10], cx: &mut Context<Self>) {
        self.config.equalizer.gains_db = gains;
        self.config.equalizer.enabled = true;
        self.push_equalizer_to_audio();
        self.schedule_eq_save(cx);
        cx.notify();
    }

    fn push_equalizer_to_audio(&self) {
        self.audio.set_equalizer(self.config.equalizer.clone());
    }

    /// 均衡器保存防抖（拖动滑杆时避免频繁写盘，500ms）。
    fn schedule_eq_save(&mut self, cx: &mut Context<Self>) {
        self.eq_save_generation = self.eq_save_generation.wrapping_add(1);
        let generation = self.eq_save_generation;
        cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_millis(500)).await;
            this.update(cx, |this, cx| {
                if this.eq_save_generation != generation {
                    return;
                }
                if let Err(error) = config::save(&this.config) {
                    this.notify_error(format!("Equalizer save failed: {error:#}"));
                }
                cx.notify();
            })?;
            Ok::<_, anyhow::Error>(())
        })
        .detach();
    }

    /// 切换输出设备：持久化选择并通知音频线程；正在播放时
    /// 音频线程会在新设备上从当前位置继续，暂停意图保留。
    fn set_output_device(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        if self.config.output_device == name {
            return;
        }
        self.config.output_device = name.clone();
        if let Err(error) = config::save(&self.config) {
            self.notify_error(format!("Audio output save failed: {error:#}"));
        }
        self.cancel_next_preload();
        self.audio.set_output_device(name);
        cx.notify();
    }

    fn set_theme(
        &mut self,
        preference: ThemePreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.config.theme = preference;
        match preference {
            ThemePreference::Light => Theme::change(ThemeMode::Light, Some(window), cx),
            ThemePreference::Dark => Theme::change(ThemeMode::Dark, Some(window), cx),
            ThemePreference::System => Theme::sync_system_appearance(Some(window), cx),
        }
        if let Err(error) = config::save(&self.config) {
            self.state.status = format!("Theme save failed: {error:#}");
        }
        cx.notify();
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

    /// 持久化当前播放会话（队列、索引、位置、模式、随机历史）。
    fn save_session(&mut self) {
        let Some(api) = self.api.as_ref() else { return };
        if self.state.queue.is_empty() {
            return;
        }
        let position = self.audio.state().position;
        let (shuffle_played, shuffle_forward) = self.state.shuffle_history.snapshot();
        let api_server_id = self
            .config
            .active_server_id()
            .unwrap_or_default()
            .to_string();
        let session = PlaybackSession {
            server_url: api.base_url().to_string(),
            queue: self.state.queue.clone(),
            queue_index: self.state.queue_index,
            position_secs: position.as_secs_f64(),
            playback_modes: self.state.playback_modes,
            shuffle_played,
            shuffle_forward,
        };
        let server_id = api_server_id.to_string();
        if let Err(error) = config::save_session(Some(&server_id), &session) {
            log::warn!("failed to save playback session: {error:#}");
        }
    }

    /// 位置变化节流保存（每 5 秒一次，避免频繁写盘）。
    fn maybe_save_session(&mut self) {
        if self.state.queue.is_empty() {
            return;
        }
        let now = std::time::Instant::now();
        if now.duration_since(self.last_session_save) < Duration::from_secs(5) {
            return;
        }
        self.last_session_save = now;
        self.save_session();
    }

    fn handle_playback_end(&mut self) {
        self.sync_gapless_transition();
        let playback = self.audio.state();
        if self.state.ended_handled || self.state.now_playing.is_none() {
            return;
        }
        if let Some(error) = playback.error {
            self.cancel_next_preload();
            log::error!("playback error reported to UI: {error}");
            self.state.ended_handled = true;
            if self.schedule_reconnect(playback.failure, playback.position) {
                return;
            }
            // Opus 源解码失败时给出可操作的提示（而不是误导性的
            // “格式未识别”）：说明需要服务器转码，并指向两案。
            let undecodable_source = self.state.now_playing.as_ref().is_some_and(|song| {
                TranscodingQuality::is_undecodable_source(
                    song.suffix.as_deref(),
                    song.content_type.as_deref(),
                )
            });
            let error = if undecodable_source {
                format!(
                    "{error} Opus tracks must be transcoded by the server: enable transcoding \
                     in Navidrome, or pick a lower playback quality."
                )
            } else {
                error
            };
            self.notify_error(error);
        } else if playback.ended {
            self.state.ended_handled = true;
            self.ensure_scrobble_on_end();
            self.cache_measured_loudness(playback.loudness_db, playback.loudness_peak);
            self.play_next(true);
        }
    }

    /// 播放中断后安排一次自动重连。只有网络/服务端类失败才重试；
    /// 解码、本地输出问题以及重试次数用尽时返回 false。
    fn schedule_reconnect(&mut self, failure: Option<StreamFailure>, position: Duration) -> bool {
        let Some(song) = self.state.now_playing.clone() else {
            return false;
        };
        let Some(kind) = failure else {
            return false;
        };
        let previous = self
            .reconnect_attempts
            .as_ref()
            .filter(|(id, _, _)| id == &song.id)
            .map_or(0, |(_, count, _)| *count);
        let (attempt, delay) =
            match reconnect_decision(failure, previous, PLAYBACK_RECONNECT_ATTEMPTS) {
                ReconnectDecision::DoNotRetry => return false,
                ReconnectDecision::Exhausted => {
                    self.reconnect = None;
                    self.reconnect_attempts = None;
                    self.notify_error(format!(
                        "{} — gave up after {PLAYBACK_RECONNECT_ATTEMPTS} retries. {}",
                        kind.summary(),
                        kind.cause()
                    ));
                    return true;
                }
                ReconnectDecision::Retry { attempt, delay } => (attempt, delay),
            };
        log::warn!(
            "playback interrupted ({kind:?}); scheduling reconnect {attempt}/{PLAYBACK_RECONNECT_ATTEMPTS} in {}ms from {}ms",
            delay.as_millis(),
            position.as_millis()
        );
        self.notify_warning(format!(
            "{} — reconnecting ({attempt}/{PLAYBACK_RECONNECT_ATTEMPTS})…",
            kind.summary()
        ));
        self.reconnect_attempts = Some((song.id.clone(), attempt, position));
        self.reconnect = Some(Reconnect {
            at: std::time::Instant::now() + delay,
            attempt,
            position,
            song_id: song.id,
        });
        true
    }

    /// 由 UI 循环调用：到点后从断点重新拉流，并在播放恢复正常后清零计数。
    fn poll_playback_reconnect(&mut self) {
        let playback = self.audio.state();
        // 只有播放真的重新前进了一段（而不是刚发出 Play 就被当成成功）才清零
        // 重试计数，否则连续失败会被无限重试。
        if self.reconnect.is_none() && playback.error.is_none() {
            let recovered = self
                .reconnect_attempts
                .as_ref()
                .is_some_and(|(id, _, failed_at)| {
                    self.state
                        .now_playing
                        .as_ref()
                        .is_some_and(|song| &song.id == id)
                        && playback.position > failed_at.saturating_add(RECOVERY_CONFIRMATION)
                });
            if recovered {
                log::info!("playback recovered; clearing reconnect attempts");
                self.reconnect_attempts = None;
            }
        }
        let Some(plan) = self.reconnect.as_ref() else {
            return;
        };
        if std::time::Instant::now() < plan.at {
            return;
        }
        let Some(plan) = self.reconnect.take() else {
            return;
        };
        // 用户已经切歌或停止时宁可不重连。
        let still_current = self
            .state
            .now_playing
            .as_ref()
            .is_some_and(|song| song.id == plan.song_id);
        if !still_current || self.api.is_none() {
            self.reconnect_attempts = None;
            return;
        }
        if let Some(index) = self.state.queue_index {
            log::info!(
                "reconnecting playback; attempt={} index={index} position_ms={}",
                plan.attempt,
                plan.position.as_millis()
            );
            self.play_queue_index_at(index, plan.position);
            self.state.ended_handled = false;
        }
    }

    fn update_active_lyric(&mut self) {
        if self.state.view != View::NowPlaying {
            self.lyrics_scroll_target = None;
            return;
        }

        let active = self
            .state
            .lyrics
            .as_ref()
            .filter(|lyrics| lyrics.is_synced())
            .and_then(|lyrics| lyrics.active_line_index(self.audio.state().position));

        if active != self.active_lyric_index {
            self.active_lyric_index = active;
            if let Some(index) = active {
                let item_index = index + 1;
                if let Some(item_bounds) = self.lyrics_scroll_handle.bounds_for_item(item_index) {
                    let viewport_bounds = self.lyrics_scroll_handle.bounds();
                    self.lyrics_scroll_target =
                        Some(viewport_bounds.center().y - item_bounds.center().y);
                } else {
                    self.lyrics_scroll_handle.scroll_to_item(item_index);
                }
            }
        }

        if let Some(target) = self.lyrics_scroll_target {
            let current = self.lyrics_scroll_handle.offset();
            let distance = f32::from(target - current.y);
            if distance.abs() <= 0.75 {
                self.lyrics_scroll_handle
                    .set_offset(point(current.x, target));
                self.lyrics_scroll_target = None;
            } else {
                self.lyrics_scroll_handle
                    .set_offset(point(current.x, current.y + px(distance * 0.24)));
            }
        }
    }

    /// 侧边栏底部的折叠/展开按钮；状态持久化到配置。
    fn sidebar_collapse_toggle(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let collapsed = self.config.sidebar_collapsed;
        let button = Button::new("sidebar-collapse-toggle")
            .icon(if collapsed {
                KitIcon::PanelLeftOpen
            } else {
                KitIcon::PanelLeftClose
            })
            .tooltip(if collapsed {
                "Expand sidebar"
            } else {
                "Collapse sidebar"
            })
            .ghost()
            .small()
            .on_click(cx.listener(|this, _, _, cx| {
                this.config.sidebar_collapsed = !this.config.sidebar_collapsed;
                if let Err(error) = config::save(&this.config) {
                    this.notify_error(format!("Settings save failed: {error:#}"));
                }
                cx.notify();
            }));
        if collapsed {
            div()
                .mt_auto()
                .w_full()
                .flex()
                .justify_center()
                .pt_2()
                .child(button)
                .into_any_element()
        } else {
            div()
                .mt_auto()
                .w_full()
                .pt_2()
                .child(button)
                .into_any_element()
        }
    }

    fn nav_button(
        &self,
        view: View,
        label: &'static str,
        icon: impl IconNamed,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let selected = self.state.view == view
            || matches!(
                (view, self.state.view),
                (View::Artists, View::ArtistDetail)
                    | (View::Albums, View::AlbumDetail)
                    | (View::Genres, View::GenreDetail)
                    | (View::Playlists, View::PlaylistDetail)
            );
        let collapsed = self.config.sidebar_collapsed;
        let button = Button::new(SharedString::from(format!("nav-{label}")))
            .icon(icon)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.state.settings_open = false;
                this.state.view = view;
                match view {
                    View::Artists if this.state.artists.is_empty() => this.load_artists(),
                    View::Albums if this.state.albums.is_empty() => this.load_albums(),
                    View::Genres if this.state.genres.is_empty() => this.load_genres(),
                    View::Playlists if this.state.playlists.is_empty() => this.load_playlists(),
                    View::Favorites => this.load_favorites(),
                    _ => {}
                }
                cx.notify();
            }));
        if collapsed {
            // 折叠态：图标按钮居中、拉高加大点击区，标签退化为 tooltip。
            div()
                .w_full()
                .flex()
                .justify_center()
                .child(
                    button
                        .tooltip(label)
                        .ghost()
                        .selected(selected)
                        .w_full()
                        .h(px(32.0)),
                )
                .into_any_element()
        } else {
            button
                .label(label)
                .justify_start()
                .ghost()
                .selected(selected)
                .w_full()
                .into_any_element()
        }
    }

    /// 侧边栏顶部的服务器胶囊：单行全圆角胶囊（状态圆点 + 当前
    /// 服务器名 + 下拉箭头），点击打开设置（切换/新增/删除都在
    /// 那里）。折叠态只显示图标按钮。
    fn render_sidebar_server(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let connected = self.state.server.is_some();
        let label = self.active_server_label();
        let status = if self.state.loading {
            "Connecting…"
        } else if connected {
            "Connected"
        } else {
            "Offline"
        };
        let server_count = self.config.servers.len();
        let tooltip = if server_count > 1 {
            format!("{label} — {status}\n{server_count} servers configured · click to switch")
        } else {
            format!("{label} — {status}\nClick to manage servers")
        };
        let icon = if connected {
            AppIcon::Server
        } else {
            AppIcon::ServerOff
        };

        if self.config.sidebar_collapsed {
            return div()
                .w_full()
                .flex()
                .justify_center()
                .py_1()
                .child(
                    Button::new("sidebar-server")
                        .icon(icon)
                        .tooltip(tooltip)
                        .ghost()
                        .selected(self.state.settings_open)
                        .w_full()
                        .h(px(32.0))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.state.settings_open = true;
                            this.refresh_output_devices(cx);
                            cx.notify();
                        })),
                )
                .into_any_element();
        }

        // 已连接：整颗填充当前播放专辑的主色（与“正在播放”页播放按钮
        // 完全同一套配色管线：now_playing_accent 从封面提取、无封面时
        // 回退 theme().info——注意默认主题的 accent token 是灰，直接用
        // 它只会得到灰色胶囊；文字用 accent_foreground 按对比度自动选
        // 黑/白）。连接中/离线是瞬态，保持中性灰底 + 边框。
        let (pill_bg, pill_hover_bg, pill_border, pill_fg, dot_color) = if connected {
            let accent = self.now_playing_accent(cx);
            let fg = accent_foreground(accent);
            (
                accent,
                adjust_lightness(accent, 0.05),
                hsla(0.0, 0.0, 0.0, 0.0),
                fg,
                fg,
            )
        } else if self.state.loading {
            (
                cx.theme().secondary,
                cx.theme().accent.opacity(0.10),
                cx.theme().border,
                cx.theme().foreground,
                cx.theme().accent.opacity(0.45),
            )
        } else {
            (
                cx.theme().secondary,
                cx.theme().accent.opacity(0.10),
                cx.theme().border,
                cx.theme().foreground,
                cx.theme().border,
            )
        };

        // 胶囊包在整行居中的容器里：名字短时胶囊贴合内容（不像以前那样
        // 撑满整行），长名在整行宽度内截断。
        h_flex()
            .w_full()
            .justify_center()
            .py_1()
            .child(
                h_flex()
                    .id("sidebar-server")
                    .max_w_full()
                    .h(px(30.0))
                    .items_center()
                    .gap_2()
                    .px_4()
                    .rounded_full()
                    .when(!connected, |this| this.border_1().border_color(pill_border))
                    .bg(pill_bg)
                    .cursor_pointer()
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                    .hover(move |style| style.bg(pill_hover_bg))
                    .child(div().flex_none().size(px(6.0)).rounded_full().bg(dot_color))
                    .child(
                        div()
                            .min_w_0()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(pill_fg)
                            .truncate()
                            .child(label),
                    )
                    .child(
                        Icon::new(AppIcon::ChevronDown)
                            .with_size(px(12.0))
                            .text_color(pill_fg.opacity(0.85)),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.settings_open = true;
                        this.refresh_output_devices(cx);
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    fn favorite_button(&self, kind: FavoriteKind, id: &str, cx: &Context<Self>) -> Button {
        let key = FavoriteKey::new(kind, id);
        let starred = self.state.favorite_ids.contains(&key);
        let pending = self.state.pending_favorites.contains(&key);
        let kind_name = match kind {
            FavoriteKind::Artist => "artist",
            FavoriteKind::Album => "album",
            FavoriteKind::Song => "song",
        };

        Button::new(SharedString::from(format!("favorite-{kind_name}-{id}")))
            .icon(if starred {
                AppIcon::HeartFilled
            } else {
                AppIcon::Heart
            })
            .tooltip(if starred {
                "Remove from favorites"
            } else {
                "Add to favorites"
            })
            .ghost()
            .small()
            .opacity(if pending { 0.6 } else { 1.0 })
            .rounded_full()
            .text_color(if starred {
                cx.theme().red
            } else {
                cx.theme().muted_foreground
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_favorite(key.clone());
                cx.notify();
            }))
    }

    /// 随机开关按钮：开启时高亮；与循环模式相互独立。
    fn shuffle_button(&self, cx: &Context<Self>) -> Button {
        Button::new("player-shuffle")
            .icon(AppIcon::Shuffle)
            .tooltip(if self.state.playback_modes.shuffle {
                "Shuffle on"
            } else {
                "Shuffle off"
            })
            .ghost()
            .with_size(px(30.0))
            .rounded_full()
            .selected(self.state.playback_modes.shuffle)
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_shuffle(cx);
            }))
    }

    /// 循环模式按钮：Off → All → One 循环切换，开启时高亮，
    /// 单曲循环显示专属图标。
    fn repeat_button(&self, cx: &Context<Self>) -> Button {
        let repeat = self.state.playback_modes.repeat;
        Button::new("player-repeat")
            .icon(if repeat == RepeatMode::One {
                AppIcon::RepeatOne
            } else {
                AppIcon::Repeat
            })
            .tooltip(repeat.tooltip())
            .ghost()
            .with_size(px(30.0))
            .rounded_full()
            .selected(repeat != RepeatMode::Off)
            .on_click(cx.listener(|this, _, _, cx| {
                this.cycle_repeat(cx);
            }))
    }

    fn now_playing_palette(&self, cx: &Context<Self>) -> (Hsla, Hsla) {
        self.state
            .now_playing
            .as_ref()
            .and_then(|song| song.cover_art.as_deref())
            .and_then(|cover_id| self.state.cover_palettes.get(cover_id).copied())
            .unwrap_or((cx.theme().info, cx.theme().chart_2))
    }

    fn now_playing_accent(&self, cx: &Context<Self>) -> Hsla {
        let (_, accent) = self.now_playing_palette(cx);
        let accent = if accent.s < 0.16 {
            cx.theme().info
        } else {
            accent
        };
        readable_accent(accent, cx.theme().background)
    }
}

impl Render for NavidromeApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let playback = self.audio.state();
        let cover = self
            .state
            .now_playing
            .as_ref()
            .and_then(|song| song.cover_art.as_ref())
            .and_then(|id| self.state.covers.get(id))
            .cloned()
            .unwrap_or_else(|| self.default_cover.clone());
        let animate_cover = self.state.view == View::NowPlaying
            && !self.mini_mode
            && !self.state.settings_open
            && playback.active
            && !playback.paused
            && window.is_visible()
            && window.is_window_active();
        self.vinyl_cover
            .prepare(cover, animate_cover, &self.runtime, window);
        let progress = playback
            .duration
            .filter(|duration| !duration.is_zero())
            .map(|duration| {
                (playback.position.as_secs_f32() / duration.as_secs_f32() * 100.0).clamp(0.0, 100.0)
            })
            .unwrap_or_default();
        self.playback_slider.update(cx, |slider, cx| {
            let SliderValue::Single(current) = slider.value() else {
                return;
            };
            if (current - progress).abs() > 0.5 {
                slider.set_value(progress, window, cx);
            }
        });

        if self.mini_mode {
            // Lock the mini window to its target size: if the user drags a border
            // to resize it, snap it back on the next frame.
            if let Some(target) = self.mini_target_size {
                let viewport = window.viewport_size();
                let resized = (viewport.width - target.width).abs() > px(1.0)
                    || (viewport.height - target.height).abs() > px(1.0);
                if resized {
                    window.resize(target);
                }
            }
            return self.render_mini(window, cx);
        }

        let settings_open = self.state.settings_open;

        if self.state.view == View::NowPlaying {
            let (cover_base, cover_accent) = self.now_playing_palette(cx);
            let light_background = cx.theme().background.l >= 0.5;
            let base_strength = if light_background { 0.3 } else { 0.22 };
            let accent_strength = if light_background { 0.24 } else { 0.18 };
            let background_start = cx
                .theme()
                .background
                .blend(cover_base.opacity(base_strength));
            let background_end = cx
                .theme()
                .background
                .blend(cover_accent.opacity(accent_strength));
            let titlebar_strength = if light_background { 0.26 } else { 0.2 };
            let titlebar_start = cx
                .theme()
                .title_bar
                .blend(cover_base.opacity(titlebar_strength));
            let titlebar_end = cx
                .theme()
                .title_bar
                .blend(cover_accent.opacity(titlebar_strength * 0.82));
            let titlebar_border = cx
                .theme()
                .title_bar_border
                .blend(cover_accent.opacity(0.34));
            // Reuse the playback tick instead of requesting display-rate frames
            // for a page-wide gradient (including while paused).
            let background_angle = 120.0 + (playback.position.as_secs_f32() / 22.0).fract() * 360.0;

            return v_flex()
                .size_full()
                .text_color(cx.theme().foreground)
                .child(
                    TitleBar::new()
                        .on_close_window(Self::close_button_handler(cx))
                        .bg(linear_gradient(
                            90.0,
                            linear_color_stop(titlebar_start, 0.0),
                            linear_color_stop(titlebar_end, 1.0),
                        ))
                        .border_color(titlebar_border)
                        .child(
                            h_flex()
                                .h_full()
                                .w_full()
                                .items_center()
                                .justify_between()
                                .pr_2()
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_2()
                                        // See `toggle-mini-player`: keep the clickable
                                        // part out of the title bar's drag area.
                                        .occlude()
                                        .child(
                                            Button::new("leave-now-playing")
                                                .icon(AppIcon::ArrowLeft)
                                                .tooltip("Back to library")
                                                .ghost()
                                                .small()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.leave_now_playing();
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child("Now Playing"),
                                        ),
                                )
                                .child(
                                    div().occlude().child(
                                        Button::new("now-playing-settings")
                                            .icon(AppIcon::Settings)
                                            .tooltip("Settings")
                                            .ghost()
                                            .small()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.state.settings_open = true;
                                                this.refresh_output_devices(cx);
                                                cx.notify();
                                            })),
                                    ),
                                ),
                        ),
                )
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .child(self.render_now_playing(window, cx))
                        .when(settings_open, |this| {
                            this.child(self.render_settings_overlay(cx))
                        }),
                )
                .child(self.render_player(window, cx))
                .bg(linear_gradient(
                    background_angle,
                    linear_color_stop(background_start, 0.0),
                    linear_color_stop(background_end, 1.0),
                ))
                .into_any_element();
        }

        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new()
                    .on_close_window(Self::close_button_handler(cx))
                    .child(
                        h_flex()
                            .h_full()
                            .w_full()
                            .items_center()
                            .justify_between()
                            .pr_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Navidrome Client"),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1()
                                    // On Windows the title bar is a window drag area
                                    // (`WindowControlArea::Drag`). Without this the drag
                                    // area swallows the clicks on the buttons below it.
                                    .occlude()
                                    .child(
                                        Button::new("toggle-mini-player")
                                            .icon(AppIcon::MiniPlayer)
                                            .tooltip(if self.mini_mode {
                                                "Exit mini player"
                                            } else {
                                                "Switch to mini player"
                                            })
                                            .ghost()
                                            .small()
                                            .selected(self.mini_mode)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.toggle_mini_mode(window, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("title-settings")
                                            .icon(AppIcon::Settings)
                                            .tooltip("Settings")
                                            .ghost()
                                            .small()
                                            .selected(self.state.settings_open)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.state.settings_open =
                                                    !this.state.settings_open;
                                                if this.state.settings_open {
                                                    this.refresh_output_devices(cx);
                                                }
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        v_flex()
                            .w(px(if self.config.sidebar_collapsed {
                                56.0
                            } else {
                                216.0
                            }))
                            .h_full()
                            .p_3()
                            .gap_1()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().sidebar)
                            // 当前服务器：多服务器下必须一眼可见正在浏览谁的库，
                            // 点击进入设置切换。
                            .child(self.render_sidebar_server(cx))
                            .when(!self.config.sidebar_collapsed, |this| {
                                this.child(
                                    div()
                                        .px_2()
                                        .py_2()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(cx.theme().muted_foreground)
                                        .child("LIBRARY"),
                                )
                            })
                            .child(self.nav_button(View::Home, "Home", AppIcon::Home, cx))
                            .child(self.nav_button(
                                View::Favorites,
                                "Favorites",
                                AppIcon::Heart,
                                cx,
                            ))
                            .child(self.nav_button(View::Artists, "Artists", AppIcon::Artists, cx))
                            .child(self.nav_button(View::Albums, "Albums", AppIcon::Albums, cx))
                            .child(self.nav_button(View::Genres, "Genres", AppIcon::Genres, cx))
                            .child(self.nav_button(
                                View::Playlists,
                                "Playlists",
                                AppIcon::Playlists,
                                cx,
                            ))
                            .child(self.nav_button(View::Search, "Search", KitIcon::Search, cx))
                            .child(self.sidebar_collapse_toggle(cx)),
                    )
                    .child(
                        div()
                            .id("content")
                            .flex_1()
                            .h_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.content_scroll_handle)
                            .on_scroll_wheel(cx.listener(|this, _, _, _| {
                                this.maybe_load_more_content();
                            }))
                            .p_6()
                            .child(self.render_content(window, cx)),
                    )
                    // 设置为内容区覆盖层：点击背景关闭，标题栏与播放条保持可操作。
                    .when(settings_open, |row| {
                        row.child(self.render_settings_overlay(cx))
                    }),
            )
            .child(self.render_player(window, cx))
            .into_any_element()
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
        .text_size(px(11.0))
        .text_color(cx.theme().foreground.opacity(0.74))
        .truncate()
        .child(label)
        .into_any_element()
}
