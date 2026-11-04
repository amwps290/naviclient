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
mod detail_view;
mod helpers;
mod home_view;
mod import;
mod library;
mod library_view;
mod lyrics;
mod media_controls;
mod message_polling;
mod now_playing_view;
mod platform_window;
mod playback;
mod playback_lifecycle;
mod player_view;
mod playlist_list_view;
mod playlists;
mod queue;
mod queue_view;
mod recommendations;
mod render;
mod search_view;
mod server_settings_view;
mod settings;
mod settings_view;
mod shell_view;
mod shortcut_settings_view;
mod shortcuts;
mod song_view;
mod startup;
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

impl NavidromeApp {
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
