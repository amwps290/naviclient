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
    switch::Switch,
    tooltip::Tooltip,
    v_flex, window_paddings, ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable, Theme,
    ThemeMode, TitleBar, WindowExt,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, ease_out_quint, hsla, img, linear_color_stop, linear_gradient, percentage, point, px,
    relative, rems, uniform_list, Animation, AnimationExt, App, AppContext, Context, Entity,
    FontWeight, Hsla, Image as GpuiImage, ImageFormat as GpuiImageFormat, InteractiveElement,
    IntoElement, Keystroke, MouseButton, ObjectFit, ParentElement, PathPromptOptions, Pixels,
    Render, ScrollHandle, ScrollWheelEvent, SharedString, StatefulInteractiveElement, Styled,
    StyledImage, Subscription, Transformation, UniformListScrollHandle, Window, WindowControlArea,
};
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
    Album, AlbumSort, Artist, ArtistSort, Config, EqSettings, FavoriteKey, FavoriteKind, Favorites,
    Genre, LanguagePreference, PlayModes, PlaybackQueue, PlaybackSession, Playlist, QueueEntry,
    QueueOrigin, RepeatMode, SearchResults, ServerInfo, ServerProfile, ShortcutAction, Song,
    ThemePreference, TranscodingQuality, VolumeNormalization,
};
use crate::msg::{error_message, Msg};
use crate::notify;
use crate::secrets;
use crate::single_instance;
use crate::tray::{self, TrayCommand};
use crate::vinyl::VinylCover;

#[cfg(test)]
mod tests;

mod album_details;
mod album_grid_view;
mod album_paging;
mod appearance_settings_view;
mod artist_details;
mod artist_grid_view;
mod artist_library;
mod artwork_helpers;
mod artwork_view;
mod audio_output;
mod auto_continue;
mod auto_continue_state;
mod browse_view;
mod connection;
mod content_view;
mod cover_state;
mod cover_texture_state;
mod cover_textures;
mod covers;
mod daily_mix;
mod daily_mix_state;
mod daily_mix_view;
mod desktop;
mod desktop_settings_view;
mod desktop_state;
mod detail_view;
mod equalizer;
mod equalizer_state;
mod equalizer_view;
mod favorite_button_view;
mod favorites;
mod genre_library;
mod home_feature_view;
mod home_library;
mod home_section_view;
mod home_view;
mod import;
mod import_flow;
mod import_view;
mod library;
mod library_helpers;
mod library_navigation;
mod library_playback;
mod library_requests;
mod library_skeleton_view;
mod library_view;
mod listening;
mod listening_state;
mod loudness;
mod loudness_state;
mod lyrics;
mod lyrics_state;
mod media_command_state;
mod media_commands;
mod media_controls;
mod message_polling;
mod metadata_links;
mod mini_player_view;
mod mini_window;
mod mini_window_state;
mod navigation;
mod now_playing_view;
mod paged_collection;
mod platform_window;
mod playback;
mod playback_display;
mod playback_lifecycle;
mod playback_logic;
mod playback_modes;
mod playback_occurrence;
mod playback_preload;
mod playback_reconnect;
mod playback_session;
mod playback_settings_view;
mod playback_start;
mod playback_transport;
mod player_controls_view;
mod player_progress_view;
mod player_view;
mod playlist_dialog;
mod playlist_dialog_view;
mod playlist_edit_state;
mod playlist_library;
mod playlist_list_view;
mod playlist_loading;
mod playlists;
mod polling;
mod preferences;
mod preload_state;
mod queue;
mod queue_edits;
mod queue_logic;
mod queue_navigation;
mod queue_view;
mod radio;
mod radio_state;
mod recommendations;
mod redraw_state;
mod render;
mod scrolling_text_state;
mod scrolling_text_view;
mod search;
mod search_state;
mod search_view;
mod server_cache;
mod server_connection;
mod server_edit;
mod server_form_view;
mod server_lifecycle;
mod server_list_view;
mod server_profile_edit;
mod server_profiles;
mod server_removal_state;
mod server_remove_dialog;
mod server_settings_view;
mod session_state;
mod settings_navigation_view;
mod settings_overlay;
mod settings_state;
mod settings_view;
mod shortcut_bindings;
mod shortcut_config;
mod shortcut_edit;
mod shortcut_repeat;
mod shortcut_row_view;
mod shortcut_settings_view;
mod shortcuts;
mod sidebar_view;
mod song_info_view;
mod song_list;
mod song_menu;
mod song_playback;
mod song_row_actions;
mod song_row_view;
mod song_scope;
mod song_selection;
mod song_view;
mod startup;
mod state;
mod titlebar_view;
mod view_helpers;
mod view_state;
mod volume;
mod volume_state;
mod volume_view;

use self::import::ImportUi;
pub(crate) use auto_continue_state::AutoContinueRequest;
pub(crate) use cover_state::CoverRequest;
use cover_texture_state::CoverTextures;
pub(crate) use daily_mix_state::DailyMixRequest;
use desktop_state::DesktopState;
use equalizer_state::EqualizerState;
pub(crate) use favorites::FavoriteChange;
pub(crate) use home_library::HomeAlbumSection;
use import_flow::ImportStage;
pub(crate) use library_playback::LibraryPlaybackRequest;
pub(crate) use library_requests::LibraryRequest;
use library_requests::LibraryRequests;
use mini_window_state::MiniWindowState;
pub(crate) use paged_collection::PageRequest;
use paged_collection::PageRequests;
use platform_window::*;
use playback_reconnect::PlaybackReconnect;
use playlist_dialog::PlaylistUi;
use playlist_edit_state::PlaylistEditOrigin;
use preload_state::PreloadState;
pub(crate) use radio_state::RadioRequest;
use scrolling_text_state::TextWidthCache;
use search_state::SearchFilter;
use server_removal_state::ServerRemovalState;
use session_state::SessionState;
use settings_state::{SettingsSection, ShortcutRecording};
use shortcut_repeat::ShortcutRepeatGuard;
use song_scope::SongScope;
use state::AppState;
use view_state::{QueueDrawerTab, View};
use volume_state::VolumeState;

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
    preload: PreloadState,
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
    /// Volume interactions survive library/server state resets.
    volume: VolumeState,
    lyrics_scroll_handle: ScrollHandle,
    content_scroll_handle: ScrollHandle,
    /// 设置卡片内容区的滚动句柄。
    settings_scroll: ScrollHandle,
    /// 每个歌曲列表作用域的 uniform_list 滚动句柄：视口虚拟化 + 按需预取封面。
    song_list_scrolls: RefCell<HashMap<SongScope, UniformListScrollHandle>>,
    /// 待发出的 Toast 通知：错误站点大多拿不到 Window，先入队，
    /// 由 UI 循环统一用 window.push_notification 冲刷。
    pending_notifications: Vec<(NotificationType, SharedString)>,
    cover_slots: Arc<tokio::sync::Semaphore>,
    /// Retention and pending texture/asset releases survive server state resets.
    cover_textures: RefCell<CoverTextures>,
    text_width_cache: RefCell<TextWidthCache>,
    tray_rx: Receiver<TrayCommand>,
    /// 切歌系统通知发送器（同 ID 替换防堆积）。
    track_notifier: notify::TrackNotifier,
    /// Track notification and tray deduplication survive library state resets.
    desktop: DesktopState,
    main_hwnd: Option<isize>,
    quitting: Arc<AtomicBool>,
    /// Repeated-key suppression survives library/server state resets.
    shortcut_repeat: ShortcutRepeatGuard,
    session: SessionState,
    server_cache_writer: config::ServerCacheWriter,
    /// 播放因网络/服务端问题中断后的自动重连计划。
    reconnect: PlaybackReconnect,
    _subscriptions: Vec<Subscription>,
    mini_window: MiniWindowState,
    /// 均衡器 10 段滑杆状态（设置页）。
    eq_sliders: Vec<Entity<SliderState>>,
    /// Equalizer edits and persistence survive library/server state resets.
    equalizer: EqualizerState,
    /// 专辑与流派分页请求编号；切服务器时保留以隔离迟到响应。
    album_page_requests: PageRequests,
    /// 媒体库请求编号；切服务器时保留以隔离迟到响应。
    library_requests: LibraryRequests,
    /// 删除确认目标独立保存，不随服务器和页面状态重建而丢失。
    server_removal: ServerRemovalState,
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
