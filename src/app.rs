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
mod platform_window;
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
mod state;
use self::import::{ImportStage, ImportUi};
use helpers::*;
use platform_window::*;
use playlists::{PlaylistUi, SongMenuContext};
pub(crate) use state::SongListMulti;
use state::{AppState, Reconnect, SettingsSection, RECOVERY_CONFIRMATION};

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
