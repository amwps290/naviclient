use prelude::*;

#[cfg(test)]
mod tests;

pub(in crate::app) mod prelude;

mod covers;
mod desktop;
mod home;
mod import;
mod library;
mod playback;
mod player;
mod playlists;
mod queue;
mod recommendations;
mod search;
mod server;
mod settings;
mod shell;
mod shortcuts;
mod songs;
mod sound;
mod widgets;

pub(crate) use covers::state::CoverRequest;
pub(crate) use home::albums::HomeAlbumSection;
pub(crate) use library::favorites::FavoriteChange;
pub(crate) use library::paged_collection::PageRequest;
pub(crate) use library::playback::LibraryPlaybackRequest;
pub(crate) use library::requests::LibraryRequest;
pub(crate) use recommendations::auto_continue_state::AutoContinueRequest;
pub(crate) use recommendations::daily_mix_state::DailyMixRequest;
pub(crate) use recommendations::radio_state::RadioRequest;

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
