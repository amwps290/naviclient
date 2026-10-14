use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::{
    box_shadow,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{ContextMenuExt, DropdownMenu},
    scroll::{Scrollbar, ScrollableElement},
    slider::{Slider, SliderEvent, SliderState, SliderValue},
    tooltip::Tooltip,
    v_flex, window_paddings, ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable, Theme,
    ThemeMode, TitleBar, WindowExt,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, ease_out_quint, hsla, img, linear_color_stop, linear_gradient, percentage, point, px,
    relative, rems, size, Animation, AnimationExt, AppContext, Context, Entity, FontWeight, Hsla,
    Image as GpuiImage, ImageFormat as GpuiImageFormat, InteractiveElement, IntoElement, Keystroke,
    Modifiers, MouseButton, ObjectFit, ParentElement, PathPromptOptions, Pixels, Render,
    ScrollHandle, ScrollWheelEvent, SharedString, Size, StatefulInteractiveElement, Styled,
    StyledImage, Subscription, Transformation, UniformListScrollHandle, Window, WindowControlArea,
    uniform_list,
};
use rand::{seq::SliceRandom, thread_rng};
use smol::Timer;
use tokio::runtime::Runtime;

use crate::api::{format_duration, Api};
use crate::assets::{AppIcon, PlayerIcon};
use crate::audio::{format_playback, AudioHandle, PlaybackState, TrackTransition};
use crate::config;
use crate::media_session::{self, MediaCommand, MediaSession, MediaUpdate};
use crate::models::{
    Album, Artist, Config, FavoriteKey, FavoriteKind, Favorites, Lyrics, PlaybackMode,
    PlaybackSession, Playlist, SearchResults, ServerInfo, Song, ThemePreference,
    TranscodingQuality, VolumeNormalization,
};
use crate::msg::{error_message, DecodedCover, Msg};
use crate::single_instance;
use crate::tray::{self, TrayCommand};
use crate::vinyl::VinylCover;

mod playlists;
use playlists::{PlaylistUi, SongMenuContext};

// The mini player content target size. On Linux, the client-side window shadow
// (gpui-component's WindowBorder) pads each edge of the window, so the actual
// drawable area is smaller than the outer window size. enter_mini_mode adds the
// window paddings to keep the content at this size on every platform.
const MINI_WINDOW_WIDTH: Pixels = px(200.0);
const MINI_WINDOW_HEIGHT: Pixels = px(50.0);
const ALBUM_PAGE_SIZE: usize = 100;
/// 歌曲行高：60px 内容 + 1px 顶部边框。uniform_list 限高模式和封面预取都依赖它。
const SONG_ROW_HEIGHT: f32 = 61.0;

fn item_instance_key(scope: &str, kind: &str, index: usize, id: &str) -> String {
    format!("{scope}/{kind}/{index}/{id}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Home,
    Favorites,
    Artists,
    Albums,
    Playlists,
    Search,
    ArtistDetail,
    AlbumDetail,
    PlaylistDetail,
    NowPlaying,
    Queue,
}

impl PlaybackMode {
    fn next(self) -> Self {
        match self {
            Self::Sequential => Self::RepeatAll,
            Self::RepeatAll => Self::RepeatOne,
            Self::RepeatOne => Self::Shuffle,
            Self::Shuffle => Self::Sequential,
        }
    }

    fn tooltip(self) -> &'static str {
        match self {
            Self::Sequential => "Play sequentially",
            Self::RepeatAll => "Repeat all",
            Self::RepeatOne => "Repeat current track",
            Self::Shuffle => "Shuffle queue",
        }
    }
}

#[derive(Debug, Default)]
struct ShuffleHistory {
    played: Vec<usize>,
    forward: Vec<usize>,
}

impl ShuffleHistory {
    fn start(&mut self, index: usize) {
        self.played = vec![index];
        self.forward.clear();
    }

    /// 前进到 next：丢弃回退暂存，记录已播放序列。
    fn advance(&mut self, next: usize) {
        self.forward.clear();
        self.played.push(next);
    }

    /// 回退到上一首：返回真正播放过的上一首；无可回退时返回 None。
    fn previous(&mut self) -> Option<usize> {
        if self.played.len() < 2 {
            return None;
        }
        let current = self.played.pop()?;
        self.forward.push(current);
        self.played.last().copied()
    }

    /// 回退后恢复前进：返回暂存的下一首；无暂存时返回 None。
    fn restore_forward(&mut self) -> Option<usize> {
        let index = self.forward.pop()?;
        self.played.push(index);
        Some(index)
    }

    /// 最近播放过的 index（用于随机排除，避免短时重复）。
    fn recent(&self, max: usize) -> impl Iterator<Item = usize> + '_ {
        self.played.iter().rev().take(max).copied()
    }

    /// 从持久化快照恢复。
    fn restore(played: Vec<usize>, forward: Vec<usize>) -> Self {
        Self { played, forward }
    }

    /// 导出快照用于持久化。
    fn snapshot(&self) -> (Vec<usize>, Vec<usize>) {
        (self.played.clone(), self.forward.clone())
    }

    fn remap(&mut self, mut map: impl FnMut(usize) -> Option<usize>) {
        self.played = self.played.iter().copied().filter_map(&mut map).collect();
        self.forward = self.forward.iter().copied().filter_map(map).collect();
    }
}

#[derive(Clone, Copy)]
struct NextPreload {
    id: u64,
    index: usize,
    quality: TranscodingQuality,
}

fn should_preload_next(playback: &PlaybackState) -> bool {
    playback.active
        && !playback.paused
        && !playback.ended
        && playback.error.is_none()
        && playback.fully_buffered
        && playback.duration.is_some_and(|duration| {
            let remaining = duration.saturating_sub(playback.position);
            !remaining.is_zero() && remaining <= Duration::from_secs(15)
        })
}

fn next_queue_index(
    mode: PlaybackMode,
    current: usize,
    len: usize,
    natural_end: bool,
    shuffle_forward: Option<usize>,
    reserved: Option<usize>,
    random: impl FnOnce() -> usize,
) -> Option<usize> {
    if current >= len {
        return None;
    }
    match mode {
        PlaybackMode::Shuffle => shuffle_forward
            .or(reserved)
            .or_else(|| Some(random()))
            .filter(|i| *i < len),
        PlaybackMode::RepeatOne if natural_end => Some(current),
        PlaybackMode::RepeatOne | PlaybackMode::RepeatAll => {
            advance_index(current, len, true, true)
        }
        PlaybackMode::Sequential => advance_index(current, len, true, false),
    }
}

/// 计算顺序（非随机）切歌的目标 index；`wraps` 为 true 时队列首尾循环，否则在边界停止。
fn advance_index(current: usize, len: usize, forward: bool, wraps: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    if forward {
        if current + 1 < len {
            Some(current + 1)
        } else if wraps {
            Some(0)
        } else {
            None
        }
    } else if current > 0 {
        Some(current - 1)
    } else if wraps {
        Some(len - 1)
    } else {
        None
    }
}

/// 从队列移除 `index`（`len_after` 为删除后的长度）后，返回修正后的当前索引。
/// 删除正在播放的歌曲时，回落到原位置的下一首；队列被清空时返回 `None`。
fn queue_index_after_remove(
    index: usize,
    current: Option<usize>,
    len_after: usize,
) -> Option<usize> {
    let current = current?;
    if len_after == 0 {
        return None;
    }
    if index < current {
        Some(current - 1)
    } else if index == current {
        Some(current.min(len_after - 1))
    } else {
        Some(current)
    }
}

/// 将队列项从 `from` 移到 `to` 后，返回修正后的当前索引。
fn queue_index_after_move(from: usize, to: usize, current: usize) -> usize {
    if from == current {
        return to;
    }
    let current_after_remove = if from < current { current - 1 } else { current };
    if current_after_remove >= to {
        current_after_remove + 1
    } else {
        current_after_remove
    }
}

/// 判断播放是否达到有效播放阈值（满足其一即可）：播放时长达到歌曲总长的 50%，或达到 4 分钟。
fn should_scrobble(position: Duration, duration: Option<Duration>) -> bool {
    if position.as_secs() >= 4 * 60 {
        return true;
    }
    match duration {
        Some(duration) if duration > Duration::ZERO => {
            position.as_secs_f64() >= duration.as_secs_f64() * 0.5
        }
        _ => false,
    }
}

/// 根据 ReplayGain 元数据与标准化模式计算增益系数（1.0 = 不变）。
/// 正增益（dB）表示歌曲偏轻需放大，负增益表示偏响需降低；结合峰值做防削波。
fn replay_gain_factor(
    mode: VolumeNormalization,
    track_gain_db: Option<f32>,
    track_peak: Option<f32>,
    album_gain_db: Option<f32>,
    album_peak: Option<f32>,
) -> f32 {
    let (gain_db, peak) = match mode {
        VolumeNormalization::Off => return 1.0,
        VolumeNormalization::Track => (track_gain_db, track_peak),
        VolumeNormalization::Album => (album_gain_db, album_peak),
    };
    let Some(gain_db) = gain_db.filter(|gain| gain.is_finite()) else {
        return 1.0;
    };
    let mut factor = 10f32.powf(gain_db / 20.0);
    // 防削波：有峰值元数据时，增益不能把输出峰值推过 1.0。
    if let Some(peak) = peak.filter(|peak| peak.is_finite() && *peak > 0.0) {
        factor = factor.min(1.0 / peak);
    }
    factor.clamp(0.1, 4.0)
}

/// 本地响度标准化的目标 RMS（dBFS）。
const LOUDNESS_TARGET_DB: f32 = -16.0;

/// 依据本地测量的响度与峰值计算增益系数（1.0 = 不变）。
fn loudness_gain_factor(measured_rms_db: f32, peak: f32) -> f32 {
    let gain_db = LOUDNESS_TARGET_DB - measured_rms_db;
    let mut factor = 10f32.powf(gain_db / 20.0);
    if peak > 0.0 && peak.is_finite() {
        factor = factor.min(1.0 / peak);
    }
    factor.clamp(0.1, 4.0)
}

/// 计算专辑分页的 offset；相邻页之间连续、不重叠。
fn album_page_offset(page: usize, page_size: usize) -> u32 {
    (page as u32) * (page_size as u32)
}

/// 键盘快捷键动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shortcut {
    TogglePlayback,
    Previous,
    Next,
    SeekBack,
    SeekForward,
    VolumeUp,
    VolumeDown,
    ToggleMute,
    ToggleNowPlaying,
    ToggleQueue,
    CloseOverlays,
    FocusSearch,
    None,
}

/// 将按键映射为快捷键动作（纯逻辑，便于测试）。
fn match_shortcut(key: &str, mods: Modifiers) -> Shortcut {
    if mods.control && !mods.alt && !mods.shift && !mods.platform {
        match key {
            "f" => return Shortcut::FocusSearch,
            "left" => return Shortcut::Previous,
            "right" => return Shortcut::Next,
            _ => {}
        }
    }
    if mods.modified() {
        return Shortcut::None;
    }
    match key {
        "space" => Shortcut::TogglePlayback,
        "left" => Shortcut::SeekBack,
        "right" => Shortcut::SeekForward,
        "up" => Shortcut::VolumeUp,
        "down" => Shortcut::VolumeDown,
        "m" => Shortcut::ToggleMute,
        "l" => Shortcut::ToggleNowPlaying,
        "q" => Shortcut::ToggleQueue,
        "escape" => Shortcut::CloseOverlays,
        _ => Shortcut::None,
    }
}

/// 计算快进/快退的目标位置，不越过歌曲起点与终点。
fn seek_target(position: Duration, duration: Option<Duration>, delta_secs: i64) -> Duration {
    let duration = duration.unwrap_or_default();
    if delta_secs >= 0 {
        (position + Duration::from_secs(delta_secs as u64)).min(duration)
    } else {
        position.saturating_sub(Duration::from_secs((-delta_secs) as u64))
    }
}

/// 在后台线程解码封面：按需提取调色板并构造 GPUI 图片对象，避免阻塞 UI 线程。
fn decode_cover(bytes: Vec<u8>, want_palette: bool) -> DecodedCover {
    let decode_start = std::time::Instant::now();
    let palette = if want_palette {
        extract_cover_palette(&bytes)
    } else {
        None
    };
    let image = image::guess_format(&bytes)
        .ok()
        .and_then(gpui_image_format)
        .map(|format| Arc::new(GpuiImage::from_bytes(format, bytes)));
    let decode_elapsed = decode_start.elapsed();
    if decode_elapsed > Duration::from_millis(20) {
        log::debug!(
            "cover decoded on background thread in {decode_elapsed:?} palette={want_palette}"
        );
    }
    DecodedCover { palette, image }
}

struct AppState {
    server: Option<ServerInfo>,
    view: View,
    loading: bool,
    artists: Vec<Artist>,
    artists_visible: usize,
    albums: Vec<Album>,
    albums_loading: bool,
    albums_exhausted: bool,
    albums_page: usize,
    recent_albums: Vec<Album>,
    recent_albums_loading: bool,
    frequent_albums: Vec<Album>,
    frequent_albums_loading: bool,
    random_albums: Vec<Album>,
    random_albums_loading: bool,
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
    error: Option<String>,
    settings_open: bool,
    view_before_now_playing: View,
    playback_mode: PlaybackMode,
    shuffle_history: ShuffleHistory,
    ended_handled: bool,
    now_playing_reported: Option<String>,
    scrobbled_song_id: Option<String>,
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
            albums: Vec::new(),
            albums_loading: false,
            albums_exhausted: false,
            albums_page: 0,
            recent_albums: Vec::new(),
            recent_albums_loading: false,
            frequent_albums: Vec::new(),
            frequent_albums_loading: false,
            random_albums: Vec::new(),
            random_albums_loading: false,
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
            error: None,
            settings_open: false,
            view_before_now_playing: View::Home,
            playback_mode: PlaybackMode::default(),
            shuffle_history: ShuffleHistory::default(),
            ended_handled: true,
            now_playing_reported: None,
            scrobbled_song_id: None,
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
    /// 每个歌曲列表作用域的 uniform_list 滚动句柄：视口虚拟化 + 按需预取封面。
    song_list_scrolls: RefCell<HashMap<&'static str, UniformListScrollHandle>>,
    cover_slots: Arc<tokio::sync::Semaphore>,
    title_width_cache: RefCell<HashMap<(u32, FontWeight, SharedString), Pixels>>,
    tray_rx: Receiver<TrayCommand>,
    main_hwnd: Option<isize>,
    quitting: Arc<AtomicBool>,
    last_shortcut_key: String,
    last_shortcut_at: std::time::Instant,
    resume_position: Option<Duration>,
    last_session_save: std::time::Instant,
    active_lyric_index: Option<usize>,
    _subscriptions: Vec<Subscription>,
    mini_mode: bool,
    restore_size: Option<Size<Pixels>>,
    restore_maximized: bool,
    mini_target_size: Option<Size<Pixels>>,
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
        let config = config::load();
        let api = Api::new(&config.server_url, &config.username, &config.password).ok();
        let (tx, rx) = mpsc::channel();
        let runtime = Runtime::new().expect("failed to create Tokio runtime");
        let media_session = MediaSession::new(runtime.handle());
        let audio_cache_dir = config::audio_cache_dir(&config);
        let audio = AudioHandle::start(audio_cache_dir).expect("failed to start audio worker");
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
                .default_value(config.password.clone())
                .masked(true)
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
            song_list_scrolls: RefCell::new(HashMap::new()),
            cover_slots: Arc::new(tokio::sync::Semaphore::new(4)),
            title_width_cache: RefCell::new(HashMap::new()),
            tray_rx: mpsc::channel().1,
            main_hwnd: None,
            quitting: Arc::new(AtomicBool::new(false)),
            last_shortcut_key: String::new(),
            last_shortcut_at: std::time::Instant::now(),
            resume_position: None,
            last_session_save: std::time::Instant::now(),
            active_lyric_index: None,
            _subscriptions: Vec::new(),
            mini_mode: false,
            restore_size: None,
            restore_maximized: false,
            mini_target_size: None,
        };
        app.state.loudness_cache = config::load_loudness();
        app.state.playback_mode = app.config.playback_mode;
        app.audio.set_volume(initial_volume);

        // 恢复上次播放会话（仅服务器匹配时；不自动播放，点播放时从保存位置继续）。
        if let Some(session) = config::load_session() {
            let server_matches = app
                .api
                .as_ref()
                .map(|api| api.base_url() == session.server_url)
                .unwrap_or(false);
            if server_matches && !session.queue.is_empty() {
                let index = session
                    .queue_index
                    .filter(|index| *index < session.queue.len());
                let restored_index = index;
                app.state.queue = session.queue;
                app.state.queue_index = index;
                app.state.playback_mode = session.playback_mode;
                app.config.playback_mode = session.playback_mode;
                app.state.shuffle_history =
                    ShuffleHistory::restore(session.shuffle_played, session.shuffle_forward);
                if let Some(index) = restored_index {
                    let song = app.state.queue.get(index).cloned();
                    if let Some(song) = song {
                        app.state.now_playing = Some(song.clone());
                        app.resume_position =
                            Some(Duration::from_secs_f64(session.position_secs.max(0.0)));
                        app.ensure_cover(song.cover_art.as_deref(), true);
                    }
                }
                log::info!(
                    "restored playback session; queue={} index={restored_index:?} position={:.1}s",
                    app.state.queue.len(),
                    session.position_secs
                );
            }
        }

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
                true
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
                        let mut changed = this.poll_tray_commands();
                        changed |= this.poll_media_commands(window, cx);
                        // 单实例激活信号：显示并置前窗口。
                        if single_instance::poll_activation() {
                            if let Some(hwnd) = this.main_hwnd {
                                show_main_window(hwnd);
                            }
                        }
                        this.maybe_save_session();
                        changed |= this.poll_messages(window, cx);
                        this.handle_playback_end();
                        this.maybe_preload_next();
                        this.maybe_scrobble_current();
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

    fn refresh_library(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.state.loading = true;
        self.state.status = "Connecting to server...".to_string();
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::Ping(api.ping().await.map_err(error_message)));
        });
        self.load_artists();
        self.load_albums();
        self.load_playlists();
        self.load_favorites();
        self.load_recent_albums();
        self.load_frequent_albums();
        self.load_random_albums();
    }

    fn load_artists(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.state.loading = true;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::Artists(api.artists().await.map_err(error_message)));
        });
    }

    fn load_albums(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.state.loading = true;
        self.state.albums_loading = true;
        self.state.albums_exhausted = false;
        self.state.albums_page = 0;
        self.state.albums.clear();
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::Albums(
                api.albums(ALBUM_PAGE_SIZE as u32, 0)
                    .await
                    .map_err(error_message),
            ));
        });
    }

    /// 加载最近播放的专辑（Home 页展示）。
    fn load_recent_albums(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.state.recent_albums_loading = true;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::RecentAlbums(
                api.albums_recent(30).await.map_err(error_message),
            ));
        });
    }

    /// 加载播放次数最多的专辑（Home 页展示）。
    fn load_frequent_albums(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.state.frequent_albums_loading = true;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::FrequentAlbums(
                api.albums_frequent(30).await.map_err(error_message),
            ));
        });
    }

    /// 加载随机专辑（Home 页展示）。
    fn load_random_albums(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.state.random_albums_loading = true;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::RandomAlbums(
                api.albums_random(30).await.map_err(error_message),
            ));
        });
    }

    fn load_more_albums(&mut self) {
        let Some(api) = self.api.clone() else { return };
        if self.state.albums_loading || self.state.albums_exhausted {
            return;
        }
        self.state.albums_loading = true;
        let offset = album_page_offset(self.state.albums_page, ALBUM_PAGE_SIZE);
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::Albums(
                api.albums(ALBUM_PAGE_SIZE as u32, offset)
                    .await
                    .map_err(error_message),
            ));
        });
    }

    fn maybe_load_more_content(&mut self) {
        // 长列表增量渲染 / 分页：接近底部时追加。
        // 歌曲列表已改为 uniform_list 视口虚拟化，不再需要逐批追加行。
        let max = self.content_scroll_handle.max_offset().y;
        // GPUI stores the translated content position as a negative offset.
        let offset = -self.content_scroll_handle.offset().y;
        if max <= px(0.0) || f32::from(max - offset) >= 600.0 {
            return;
        }
        match self.state.view {
            View::Albums => self.load_more_albums(),
            View::Artists if self.state.artists_visible < self.state.artists.len() => {
                self.state.artists_visible =
                    (self.state.artists_visible + 300).min(self.state.artists.len());
            }
            View::Playlists if self.state.playlists_visible < self.state.playlists.len() => {
                self.state.playlists_visible =
                    (self.state.playlists_visible + 100).min(self.state.playlists.len());
            }
            _ => {}
        }
    }

    /// 歌曲列表作用域对应的歌曲切片；列表滚动 / 事件处理时重新解析，
    /// 避免在每次渲染时克隆整个列表。
    fn songs_for_scope(&self, scope: &str) -> Option<&[Song]> {
        match scope {
            "album-detail" => Some(&self.state.current_songs),
            "playlist-detail" => Some(&self.state.playlist_songs),
            "favorites" => Some(&self.state.favorites.songs),
            "search" => Some(&self.state.search_results.as_ref()?.songs),
            "queue" => Some(&self.state.queue),
            _ => None,
        }
    }

    fn song_for_scope(&self, scope: &str, index: usize) -> Option<&Song> {
        self.songs_for_scope(scope)?.get(index)
    }

    /// 点击作用域列表的第 index 行：从当前列表重建队列并从该行开始播放。
    /// 队列作用域只定位到该队列位置，不重建队列。
    fn play_song_at_scope(&mut self, scope: &str, index: usize) {
        let Some(songs) = self.songs_for_scope(scope) else {
            return;
        };
        if index >= songs.len() {
            return;
        }
        let replacement = if scope == "queue" {
            None
        } else {
            Some(songs.to_vec())
        };
        self.cancel_next_preload();
        if let Some(songs) = replacement {
            self.state.queue = songs;
        }
        self.state.shuffle_history.start(index);
        self.play_queue_index(index);
        self.save_session();
    }

    /// 取得（或首次创建）某作用域歌曲列表的滚动句柄。
    fn song_list_scroll(&self, scope: &'static str) -> UniformListScrollHandle {
        self.song_list_scrolls
            .borrow_mut()
            .entry(scope)
            .or_default()
            .clone()
    }

    /// 歌曲列表滚动时按需加载可见行的封面。
    fn maybe_load_visible_song_covers(&mut self) {
        let scope = match self.state.view {
            View::AlbumDetail => "album-detail",
            View::PlaylistDetail => "playlist-detail",
            View::Queue => "queue",
            View::Favorites => "favorites",
            View::Search => "search",
            _ => return,
        };
        let Some(songs) = self.songs_for_scope(scope) else {
            return;
        };
        if songs.is_empty() {
            return;
        }
        let Some(handle) = self.song_list_scrolls.borrow().get(scope).cloned() else {
            return;
        };
        let base = handle.0.borrow().base_handle.clone();
        let offset = f32::from(-base.offset().y).max(0.0);
        let viewport = f32::from(base.bounds().size.height).max(0.0);
        // 额外预取约两个视口的封面。
        let row_height = SONG_ROW_HEIGHT;
        let first = ((offset / row_height) as usize).min(songs.len());
        let lookahead = ((viewport / row_height) as usize).saturating_mul(2).max(40);
        let last = first.saturating_add(lookahead).min(songs.len());
        // 先收集需要加载的封面 id，避免在借用期间修改 self。
        let pending: Vec<String> = songs[first..last]
            .iter()
            .filter_map(|song| song.cover_art.clone())
            .filter(|cover| !self.state.requested_covers.contains(cover))
            .collect();
        if pending.is_empty() {
            return;
        }
        log::debug!(
            "queued {} visible song covers; rows={first}..{last}",
            pending.len()
        );
        for cover in pending {
            self.ensure_cover(Some(&cover), false);
        }
    }

    fn load_playlists(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.playlist_ui.list_loading = true;
        self.playlist_ui.list_request = self.playlist_ui.list_request.wrapping_add(1);
        let request = self.playlist_ui.list_request;
        let generation = self.playlist_ui.generation;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.playlists().await.map_err(error_message);
            let _ = tx.send(Msg::Playlists {
                generation,
                request,
                result,
            });
        });
    }

    fn load_favorites(&mut self) {
        let Some(api) = self.api.clone() else { return };
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::Favorites(api.favorites().await.map_err(error_message)));
        });
    }

    fn open_artist(&mut self, artist: Artist) {
        self.state.current_artist = Some(artist.clone());
        self.state.artist_albums.clear();
        self.state.view = View::ArtistDetail;
        self.ensure_cover(artist.cover_art.as_deref(), true);
        let Some(api) = self.api.clone() else { return };
        let artist_id = artist.id.clone();
        let tx = self.tx.clone();
        self.state.loading = true;
        self.spawn_future(async move {
            let result = api.artist_albums(&artist_id).await.map_err(error_message);
            let _ = tx.send(Msg::ArtistAlbums { artist_id, result });
        });
    }

    fn open_album(&mut self, album: Album) {
        self.state.current_album = Some(album.clone());
        self.state.current_songs.clear();
        self.state.view = View::AlbumDetail;
        self.ensure_cover(album.cover_art.as_deref(), true);
        let Some(api) = self.api.clone() else { return };
        let album_id = album.id.clone();
        let tx = self.tx.clone();
        self.state.loading = true;
        self.spawn_future(async move {
            let result = api.album_songs(&album_id).await.map_err(error_message);
            let _ = tx.send(Msg::AlbumSongs { album_id, result });
        });
    }

    /// 立即播放整张专辑：加载歌曲后在后台开始播放（不切换视图）。
    fn play_album(&mut self, album: Album) {
        self.state.current_album = Some(album.clone());
        self.state.pending_play_album = true;
        self.ensure_cover(album.cover_art.as_deref(), true);
        let Some(api) = self.api.clone() else { return };
        let album_id = album.id.clone();
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.album_songs(&album_id).await.map_err(error_message);
            let _ = tx.send(Msg::AlbumSongs { album_id, result });
        });
    }

    /// 立即播放艺术家的全部歌曲（聚合其所有专辑的曲目）。
    fn play_artist(&mut self, artist: Artist) {
        self.state.current_artist = Some(artist.clone());
        self.ensure_cover(artist.cover_art.as_deref(), true);
        let Some(api) = self.api.clone() else { return };
        let artist_id = artist.id.clone();
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = async {
                let albums = api.artist_albums(&artist_id).await?;
                let mut songs = Vec::new();
                for album in albums {
                    songs.extend(api.album_songs(&album.id).await?);
                }
                Ok::<_, anyhow::Error>(songs)
            }
            .await
            .map_err(error_message);
            let _ = tx.send(Msg::PlayArtistSongs(result));
        });
    }

    fn open_playlist(&mut self, playlist: Playlist) {
        self.state.current_playlist = Some(playlist.clone());
        self.state.playlist_songs.clear();
        self.state.view = View::PlaylistDetail;
        self.state.error = None;
        self.ensure_cover(playlist.cover_art.as_deref(), true);
        self.load_playlist_detail(playlist.id);
    }

    fn submit_search(&mut self, cx: &Context<Self>) {
        let query = self.search_input.read(cx).value().trim().to_string();
        if query.is_empty() {
            self.state.search_results = None;
            return;
        }
        let Some(api) = self.api.clone() else { return };
        self.state.loading = true;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.search(&query, 120, 80, 60).await.map_err(error_message);
            let _ = tx.send(Msg::Search(result));
        });
    }

    fn toggle_favorite(&mut self, key: FavoriteKey) {
        let Some(api) = self.api.clone() else { return };
        if !self.state.pending_favorites.insert(key.clone()) {
            return;
        }

        let starred = !self.state.favorite_ids.contains(&key);
        if starred {
            self.state.favorite_ids.insert(key.clone());
        } else {
            self.state.favorite_ids.remove(&key);
        }

        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.set_favorite(&key, starred).await.map_err(error_message);
            let _ = tx.send(Msg::FavoriteChanged {
                key,
                starred,
                result,
            });
        });
    }

    fn ensure_cover(&mut self, cover_id: Option<&str>, want_palette: bool) {
        let Some(id) = cover_id else { return };
        if !self.state.requested_covers.insert(id.to_string()) {
            return;
        }
        let Some(api) = self.api.clone() else { return };
        let url = match api.cover_url(id, 500) {
            Ok(url) => url,
            Err(error) => {
                self.state.error = Some(format!("{error:#}"));
                return;
            }
        };
        let id = id.to_string();
        let tx = self.tx.clone();
        let cover_slots = self.cover_slots.clone();
        self.spawn_future(async move {
            // 网络下载不限并发（IO 等待不占 CPU），只对解码限流，避免占满后台线程。
            let result = api.get_bytes(&url).await.map_err(error_message);
            let result = match result {
                Ok(bytes) => {
                    let _permit = cover_slots.acquire_owned().await;
                    Ok(decode_cover(bytes, want_palette))
                }
                Err(error) => Err(error),
            };
            let _ = tx.send(Msg::Cover { id, result });
        });
    }

    fn preload_covers(&mut self, ids: Vec<String>) {
        // 网格封面只用于显示，不需要提取调色板（调色板仅播放页/详情页配色用）。
        for id in ids {
            self.ensure_cover(Some(&id), false);
        }
    }

    fn play_song_list(&mut self, songs: &[Song], index: usize) {
        self.cancel_next_preload();
        self.state.queue = songs.to_vec();
        self.state.shuffle_history.start(index);
        self.play_queue_index(index);
        self.save_session();
    }

    /// 将单首歌曲插入到当前播放歌曲之后；未在播放时插入队首（不自动播放）。
    fn insert_next(&mut self, song: Song) {
        self.cancel_next_preload();
        let insert_at = match self.state.queue_index {
            Some(index) => (index + 1).min(self.state.queue.len()),
            None => 0,
        };
        self.state.queue.insert(insert_at, song);
        self.state
            .shuffle_history
            .remap(|i| Some(if i >= insert_at { i + 1 } else { i }));
        self.save_session();
    }

    /// 将单首歌曲追加到队列末尾（不自动播放；空队列时设为队首索引以便播放键可用）。
    fn add_to_queue(&mut self, song: Song) {
        self.cancel_next_preload();
        if self.state.queue.is_empty() {
            self.state.queue_index = Some(0);
        }
        self.state.queue.push(song);
        self.save_session();
    }

    /// 追加整个列表到队列末尾（不自动播放）。
    fn append_all(&mut self, songs: &[Song]) {
        if songs.is_empty() {
            return;
        }
        self.cancel_next_preload();
        if self.state.queue.is_empty() {
            self.state.queue_index = Some(0);
        }
        self.state.queue.extend(songs.iter().cloned());
        self.save_session();
    }

    /// 从队列移除指定项并修正当前索引。删除正在播放的歌曲时改播原位置的下一首；队列清空则停止播放。
    fn remove_from_queue(&mut self, index: usize) {
        if index >= self.state.queue.len() {
            return;
        }
        self.cancel_next_preload();
        let removed_current = self.state.queue_index == Some(index);
        self.state.queue.remove(index);
        self.state.shuffle_history.remap(|i| {
            if i == index {
                None
            } else {
                Some(if i > index { i - 1 } else { i })
            }
        });
        self.state.queue_index =
            queue_index_after_remove(index, self.state.queue_index, self.state.queue.len());
        if removed_current {
            if self.state.queue.is_empty() {
                self.stop_playback();
            } else if let Some(next) = self.state.queue_index {
                self.state.shuffle_history.advance(next);
                self.play_queue_index(next);
            }
        }
        self.save_session();
    }

    /// 清空当前播放歌曲之后的所有队列项。
    fn clear_queue_after_current(&mut self) {
        self.cancel_next_preload();
        if let Some(current) = self.state.queue_index {
            let keep = current + 1;
            if self.state.queue.len() > keep {
                self.state.queue.truncate(keep);
                self.state
                    .shuffle_history
                    .remap(|i| (i < keep).then_some(i));
                self.save_session();
            }
        }
    }

    /// 清空整个队列并停止播放。
    fn clear_queue(&mut self) {
        self.cancel_next_preload();
        self.state.queue.clear();
        self.state.queue_index = None;
        self.state.shuffle_history = ShuffleHistory::default();
        self.stop_playback();
        self.save_session();
    }

    /// 将队列项从 `from` 移动到 `to`，并修正当前索引。
    fn move_queue_item(&mut self, from: usize, to: usize) {
        self.cancel_next_preload();
        let len = self.state.queue.len();
        if from >= len || to >= len || from == to {
            return;
        }
        let song = self.state.queue.remove(from);
        self.state.queue.insert(to, song);
        self.state
            .shuffle_history
            .remap(|i| Some(queue_index_after_move(from, to, i)));
        if let Some(current) = self.state.queue_index {
            self.state.queue_index = Some(queue_index_after_move(from, to, current));
        }
        self.save_session();
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
        self.select_playing(index, &song, self.config.transcoding_quality);
        let Some(api) = self.api.clone() else {
            self.state.error = Some("Configure a server before playing".to_string());
            return;
        };
        let quality = self.config.transcoding_quality;
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
            Err(error) => self.state.error = Some(format!("{error:#}")),
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
        if let Err(error) = config::save_loudness(&self.state.loudness_cache) {
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
        if self.state.playback_mode == PlaybackMode::Shuffle
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
            self.state.playback_mode,
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
        let quality = self.config.transcoding_quality;
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
            if self.state.playback_mode == PlaybackMode::Shuffle
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

        if self.state.playback_mode == PlaybackMode::Shuffle {
            if let Some(previous) = self.state.shuffle_history.previous() {
                self.play_queue_index(previous);
            }
            return;
        }

        let wraps = matches!(
            self.state.playback_mode,
            PlaybackMode::RepeatAll | PlaybackMode::RepeatOne
        );
        let Some(next) = advance_index(index, len, forward, wraps) else {
            return;
        };
        self.play_queue_index(next);
    }

    fn cycle_playback_mode(&mut self, cx: &mut Context<Self>) {
        self.cancel_next_preload();
        self.state.playback_mode = self.state.playback_mode.next();
        self.config.playback_mode = self.state.playback_mode;
        if let Err(error) = config::save(&self.config) {
            self.state.error = Some(format!("Playback mode save failed: {error:#}"));
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
        if repeated {
            return;
        }
        match shortcut {
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
                if self.state.settings_open {
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
        let wraps = matches!(
            self.state.playback_mode,
            PlaybackMode::RepeatAll | PlaybackMode::RepeatOne
        );
        let (next, previous) = if self.state.playback_mode == PlaybackMode::Shuffle {
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
                    this.state.error = Some(format!("Volume save failed: {error:#}"));
                }
                cx.notify();
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

    fn save_settings(&mut self, cx: &Context<Self>) {
        let new_config = Config {
            server_url: self.server_input.read(cx).value().trim().to_string(),
            username: self.username_input.read(cx).value().trim().to_string(),
            password: self.password_input.read(cx).value().to_string(),
            theme: self.config.theme,
            cache_dir: self.config.cache_dir.clone(),
            transcoding_quality: self.config.transcoding_quality,
            volume: self.config.volume,
            playback_mode: self.config.playback_mode,
            volume_normalization: self.config.volume_normalization,
        };
        self.config = new_config;
        if let Err(error) = config::save(&self.config) {
            self.state.status = format!("Config save failed: {error:#}");
        }
        self.cancel_next_preload();
        self.audio.stop();
        self.state = AppState::default();
        self.playlist_ui.reset();
        self.resume_position = None;
        match Api::new(
            &self.config.server_url,
            &self.config.username,
            &self.config.password,
        ) {
            Ok(api) => {
                self.api = Some(api);
                self.refresh_library();
            }
            Err(error) => {
                self.api = None;
                self.state.status = "Invalid server URL".to_string();
                self.state.error = Some(format!("{error:#}"));
                self.state.settings_open = true;
            }
        }
    }

    fn apply_cache_directory(&mut self, cache_dir: Option<PathBuf>) {
        let effective_dir = cache_dir
            .clone()
            .unwrap_or_else(config::default_audio_cache_dir);
        if let Err(error) = fs::create_dir_all(&effective_dir) {
            self.state.error = Some(format!(
                "Unable to use cache directory {}: {error}",
                effective_dir.display()
            ));
            return;
        }

        self.config.cache_dir = cache_dir;
        if let Err(error) = config::save(&self.config) {
            self.state.error = Some(format!("Cache directory save failed: {error:#}"));
            return;
        }
        self.cancel_next_preload();
        self.audio.set_cache_directory(effective_dir.clone());
        self.state.error = None;
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
            self.state.error = Some(format!("Playback quality save failed: {error:#}"));
        } else {
            self.state.error = None;
            log::info!("transcoding quality changed; quality={}", quality.label());
        }
        cx.notify();
    }

    fn set_volume_normalization(&mut self, mode: VolumeNormalization, cx: &mut Context<Self>) {
        self.cancel_next_preload();
        self.config.volume_normalization = mode;
        if let Err(error) = config::save(&self.config) {
            self.state.error = Some(format!("Volume normalization save failed: {error:#}"));
        }
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
        let session = PlaybackSession {
            server_url: api.base_url().to_string(),
            queue: self.state.queue.clone(),
            queue_index: self.state.queue_index,
            position_secs: position.as_secs_f64(),
            playback_mode: self.state.playback_mode,
            shuffle_played,
            shuffle_forward,
        };
        if let Err(error) = config::save_session(&session) {
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

    fn poll_tray_commands(&mut self) -> bool {
        let mut changed = false;
        while let Ok(command) = self.tray_rx.try_recv() {
            changed = true;
            match command {
                TrayCommand::TogglePlayback => self.toggle_playback(),
                TrayCommand::Previous => self.skip(-1),
                TrayCommand::Next => self.skip(1),
                TrayCommand::ShowWindow => {
                    if let Some(hwnd) = self.main_hwnd {
                        show_main_window(hwnd);
                    }
                }
                TrayCommand::Quit => {
                    self.quitting.store(true, Ordering::Relaxed);
                    if let Some(hwnd) = self.main_hwnd {
                        request_window_close(hwnd);
                    }
                }
            }
        }
        changed
    }

    fn poll_messages(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        while let Ok(message) = self.rx.try_recv() {
            changed = true;
            match message {
                Msg::Ping(result) => {
                    self.state.loading = false;
                    match result {
                        Ok(info) => {
                            let server_name = info.app_name.as_deref().unwrap_or("Navidrome");
                            self.state.status = format!(
                                "{server_name} {} - {}",
                                info.version,
                                self.api
                                    .as_ref()
                                    .map(|api| api.base_url().to_string())
                                    .unwrap_or_default()
                            );
                            self.state.server = Some(info);
                        }
                        Err(error) => {
                            self.state.status = "Connection failed".to_string();
                            self.state.error = Some(error);
                        }
                    }
                }
                Msg::Artists(result) => {
                    self.state.loading = false;
                    match result {
                        Ok(artists) => {
                            let covers = artists
                                .iter()
                                .filter_map(|item| item.cover_art.clone())
                                .take(40)
                                .collect();
                            self.state.artists_visible = artists.len().min(300);
                            self.state.artists = artists;
                            self.preload_covers(covers);
                        }
                        Err(error) => self.state.error = Some(error),
                    }
                }
                Msg::ArtistAlbums { artist_id, result } => {
                    self.state.loading = false;
                    if self.state.current_artist.as_ref().map(|a| a.id.as_str())
                        == Some(artist_id.as_str())
                    {
                        match result {
                            Ok(albums) => {
                                let covers = albums
                                    .iter()
                                    .filter_map(|item| item.cover_art.clone())
                                    .collect();
                                self.state.artist_albums = albums;
                                self.preload_covers(covers);
                            }
                            Err(error) => self.state.error = Some(error),
                        }
                    }
                }
                Msg::Albums(result) => {
                    self.state.loading = false;
                    self.state.albums_loading = false;
                    let handle_start = std::time::Instant::now();
                    match result {
                        Ok(mut albums) => {
                            if albums.is_empty() {
                                self.state.albums_exhausted = true;
                            } else {
                                albums
                                    .sort_by_key(|album| album.created.clone().unwrap_or_default());
                                albums.reverse();
                                let covers: Vec<String> = albums
                                    .iter()
                                    .filter_map(|item| item.cover_art.clone())
                                    .take(80)
                                    .collect();
                                let cover_count = covers.len();
                                self.state.albums.extend(albums);
                                self.state.albums_page += 1;
                                self.preload_covers(covers);
                                log::debug!(
                                    "albums page handled in {:?}; total={} covers_queued={cover_count}",
                                    handle_start.elapsed(),
                                    self.state.albums.len(),
                                );
                            }
                        }
                        Err(error) => self.state.error = Some(error),
                    }
                }
                Msg::RecentAlbums(result) => {
                    self.state.recent_albums_loading = false;
                    match result {
                        Ok(albums) => {
                            let covers: Vec<String> = albums
                                .iter()
                                .filter_map(|item| item.cover_art.clone())
                                .take(30)
                                .collect();
                            self.state.recent_albums = albums;
                            self.preload_covers(covers);
                        }
                        Err(error) => {
                            // 非关键增强：加载失败只记录日志，不阻断其余页面。
                            log::warn!("recent albums load failed: {error}");
                        }
                    }
                }
                Msg::FrequentAlbums(result) => {
                    self.state.frequent_albums_loading = false;
                    match result {
                        Ok(albums) => {
                            let covers: Vec<String> = albums
                                .iter()
                                .filter_map(|item| item.cover_art.clone())
                                .take(30)
                                .collect();
                            self.state.frequent_albums = albums;
                            self.preload_covers(covers);
                        }
                        Err(error) => log::warn!("frequent albums load failed: {error}"),
                    }
                }
                Msg::RandomAlbums(result) => {
                    self.state.random_albums_loading = false;
                    match result {
                        Ok(albums) => {
                            let covers: Vec<String> = albums
                                .iter()
                                .filter_map(|item| item.cover_art.clone())
                                .take(30)
                                .collect();
                            self.state.random_albums = albums;
                            self.preload_covers(covers);
                        }
                        Err(error) => log::warn!("random albums load failed: {error}"),
                    }
                }
                Msg::AlbumSongs { album_id, result } => {
                    self.state.loading = false;
                    if self.state.current_album.as_ref().map(|a| a.id.as_str())
                        == Some(album_id.as_str())
                    {
                        match result {
                            Ok(mut songs) => {
                                songs.sort_by_key(|song| song.track.unwrap_or(i32::MAX));
                                // 只预加载首屏封面，滚动时按需补齐，避免一次发起几百个请求。
                                let covers = songs
                                    .iter()
                                    .filter_map(|song| song.cover_art.clone())
                                    .take(40)
                                    .collect();
                                self.state.current_songs = songs;
                                self.preload_covers(covers);
                                if self.state.pending_play_album {
                                    self.state.pending_play_album = false;
                                    let songs = self.state.current_songs.clone();
                                    self.play_song_list(&songs, 0);
                                }
                            }
                            Err(error) => self.state.error = Some(error),
                        }
                    }
                }
                Msg::Playlists {
                    generation,
                    request,
                    result,
                } => {
                    if generation != self.playlist_ui.generation
                        || request != self.playlist_ui.list_request
                    {
                        continue;
                    }
                    self.playlist_ui.list_loading = false;
                    match result {
                        Ok(playlists) => {
                            self.state.playlists_visible = playlists.len().min(50);
                            self.state.playlists = playlists;
                        }
                        Err(error) => self.playlist_read_error(error),
                    }
                }
                Msg::PlaylistEdited { generation, result } => {
                    self.finish_playlist_edit(generation, result, window, cx);
                }
                Msg::Favorites(result) => match result {
                    Ok(favorites) => {
                        let mut favorite_ids = HashSet::new();
                        favorite_ids.extend(
                            favorites
                                .artists
                                .iter()
                                .map(|artist| FavoriteKey::new(FavoriteKind::Artist, &artist.id)),
                        );
                        favorite_ids.extend(
                            favorites
                                .albums
                                .iter()
                                .map(|album| FavoriteKey::new(FavoriteKind::Album, &album.id)),
                        );
                        favorite_ids.extend(
                            favorites
                                .songs
                                .iter()
                                .map(|song| FavoriteKey::new(FavoriteKind::Song, &song.id)),
                        );
                        for key in &self.state.pending_favorites {
                            if self.state.favorite_ids.contains(key) {
                                favorite_ids.insert(key.clone());
                            } else {
                                favorite_ids.remove(key);
                            }
                        }
                        let covers = favorites
                            .albums
                            .iter()
                            .filter_map(|album| album.cover_art.clone())
                            .chain(
                                favorites
                                    .artists
                                    .iter()
                                    .filter_map(|artist| artist.cover_art.clone()),
                            )
                            .chain(
                                favorites
                                    .songs
                                    .iter()
                                    .filter_map(|song| song.cover_art.clone()),
                            )
                            .collect();
                        self.state.favorites = favorites;
                        self.state.favorite_ids = favorite_ids;
                        self.preload_covers(covers);
                    }
                    Err(error) => self.state.error = Some(error),
                },
                Msg::FavoriteChanged {
                    key,
                    starred,
                    result,
                } => {
                    self.state.pending_favorites.remove(&key);
                    if let Err(error) = result {
                        if starred {
                            self.state.favorite_ids.remove(&key);
                        } else {
                            self.state.favorite_ids.insert(key);
                        }
                        self.state.error = Some(error);
                    } else {
                        self.load_favorites();
                    }
                }
                Msg::PlaylistSongs {
                    generation,
                    request,
                    playlist_id,
                    result,
                } => {
                    if generation != self.playlist_ui.generation
                        || request != self.playlist_ui.detail_request
                    {
                        continue;
                    }
                    self.playlist_ui.detail_loading = false;
                    if self.state.current_playlist.as_ref().map(|p| p.id.as_str())
                        == Some(playlist_id.as_str())
                    {
                        match result {
                            Ok(details) => {
                                self.state.current_playlist = Some(details.playlist);
                                let songs = details.songs;
                                // 只预加载首屏封面，滚动时按需补齐。
                                let covers = songs
                                    .iter()
                                    .filter_map(|song| song.cover_art.clone())
                                    .take(40)
                                    .collect();
                                self.state.playlist_songs = songs;
                                self.preload_covers(covers);
                            }
                            Err(error) => self.state.error = Some(error),
                        }
                    }
                }
                Msg::Search(result) => {
                    self.state.loading = false;
                    match result {
                        Ok(results) => {
                            self.state.search_results = Some(results);
                        }
                        Err(error) => self.state.error = Some(error),
                    }
                }
                Msg::PlayArtistSongs(result) => match result {
                    Ok(songs) => {
                        self.play_song_list(&songs, 0);
                    }
                    Err(error) => self.state.error = Some(error),
                },
                Msg::Lyrics { song_id, result } => {
                    if self.state.now_playing.as_ref().map(|song| song.id.as_str())
                        == Some(song_id.as_str())
                    {
                        self.state.lyrics_loading = false;
                        self.active_lyric_index = None;
                        match result {
                            Ok(lyrics) => {
                                self.state.lyrics = Some(lyrics);
                                self.state.lyrics_error = None;
                            }
                            Err(error) => {
                                self.state.lyrics = None;
                                self.state.lyrics_error = Some(error);
                            }
                        }
                    }
                }
                Msg::Cover { id, result } => {
                    if let Ok(decoded) = result {
                        if let Some(palette) = decoded.palette {
                            self.state.cover_palettes.insert(id.clone(), palette);
                        }
                        if let Some(image) = decoded.image {
                            self.state.covers.insert(id, image);
                        }
                    }
                }
            }
        }
        changed
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
            self.state.error = Some(error);
        } else if playback.ended {
            self.state.ended_handled = true;
            self.ensure_scrobble_on_end();
            self.cache_measured_loudness(playback.loudness_db, playback.loudness_peak);
            self.play_next(true);
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

    fn nav_button(&self, view: View, label: &'static str, cx: &Context<Self>) -> Button {
        let selected = self.state.view == view
            || matches!(
                (view, self.state.view),
                (View::Artists, View::ArtistDetail)
                    | (View::Albums, View::AlbumDetail)
                    | (View::Playlists, View::PlaylistDetail)
            );
        let button = Button::new(SharedString::from(format!("nav-{label}")))
            .label(label)
            .w_full()
            .justify_start()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.state.settings_open = false;
                this.state.view = view;
                match view {
                    View::Artists if this.state.artists.is_empty() => this.load_artists(),
                    View::Albums if this.state.albums.is_empty() => this.load_albums(),
                    View::Playlists if this.state.playlists.is_empty() => this.load_playlists(),
                    View::Favorites => this.load_favorites(),
                    _ => {}
                }
                cx.notify();
            }));
        button.ghost().selected(selected)
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

    fn playback_mode_button(&self, cx: &Context<Self>) -> Button {
        let button = Button::new("player-playback-mode")
            .tooltip(self.state.playback_mode.tooltip())
            .ghost()
            .with_size(px(30.0))
            .rounded_full()
            .on_click(cx.listener(|this, _, _, cx| {
                this.cycle_playback_mode(cx);
            }));

        match self.state.playback_mode {
            PlaybackMode::Sequential => button.icon(AppIcon::PlaySequential),
            PlaybackMode::RepeatAll => button.icon(AppIcon::Repeat),
            PlaybackMode::RepeatOne => button.icon(AppIcon::RepeatOne),
            PlaybackMode::Shuffle => button.icon(AppIcon::Shuffle),
        }
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

    fn render_cover(
        &self,
        cover_id: Option<&str>,
        size: f32,
        _cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let cover = cover_id
            .and_then(|id| self.state.covers.get(id))
            .cloned()
            .unwrap_or_else(|| self.default_cover.clone());
        img(cover)
            .size(px(size))
            // Override GPUI's intrinsic image ratio so portrait artwork cannot grow the slot.
            .aspect_square()
            .object_fit(ObjectFit::Cover)
            .flex_none()
            .rounded_lg()
            .into_any_element()
    }

    fn render_vinyl_record(
        &self,
        cover_id: Option<&str>,
        size: f32,
        rotation_phase: f32,
        tonearm_engaged: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let cover = cover_id
            .and_then(|id| self.state.covers.get(id))
            .cloned()
            .unwrap_or_else(|| self.default_cover.clone());
        let label_size = size * 0.5;
        let label_offset = (size - label_size) * 0.5;
        let inner_cover_size = label_size - size * 0.018;
        let hole_size = size * 0.032;
        // Each tonearm layer uses (110, 110) in a 220px viewBox as its pivot.
        let tonearm_size = size * 0.94;
        let tonearm_pivot_x = size * 0.91;
        let tonearm_pivot_y = size * 0.015;
        let tonearm_left = tonearm_pivot_x - tonearm_size * 0.5;
        let tonearm_top = tonearm_pivot_y - tonearm_size * 0.5;
        let tonearm_base_size = size * 0.105;
        let tonearm_engaged_turns = 28.0 / 360.0;
        let metal_tint = self.now_playing_accent(cx);
        let metal_body = hsla(0.1, 0.025, 0.78, 1.0).blend(metal_tint.opacity(0.06));
        let cover_key = cover_id.unwrap_or("default");
        let tonearm_layer =
            |icon: AppIcon, layer: &'static str, offset_x: f32, offset_y: f32, color: Hsla| {
                Icon::new(icon)
                    .absolute()
                    .left(px(tonearm_left + offset_x))
                    .top(px(tonearm_top + offset_y))
                    .with_size(px(tonearm_size))
                    .text_color(color)
                    .with_animation(
                        SharedString::from(format!(
                            "tonearm-{layer}-{cover_key}-{tonearm_engaged}"
                        )),
                        Animation::new(Duration::from_millis(650)).with_easing(ease_out_quint()),
                        move |icon, delta| {
                            let rotation = if tonearm_engaged {
                                delta * tonearm_engaged_turns
                            } else {
                                (1.0 - delta) * tonearm_engaged_turns
                            };
                            icon.transform(Transformation::rotate(percentage(rotation)))
                        },
                    )
            };
        let tonearm_base = div()
            .absolute()
            .left(px(tonearm_pivot_x - tonearm_base_size * 0.5))
            .top(px(tonearm_pivot_y - tonearm_base_size * 0.5))
            .size(px(tonearm_base_size))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(hsla(0.1, 0.025, 0.42, 0.5))
            .bg(linear_gradient(
                132.0,
                linear_color_stop(hsla(0.1, 0.025, 0.86, 1.0), 0.0),
                linear_color_stop(
                    hsla(0.1, 0.025, 0.48, 1.0).blend(metal_tint.opacity(0.06)),
                    1.0,
                ),
            ))
            .shadow(vec![box_shadow(
                px(0.0),
                px(size * 0.006),
                px(size * 0.018),
                px(-1.0),
                hsla(0.0, 0.0, 0.0, 0.24),
            )])
            .child(
                div()
                    .size(px(tonearm_base_size * 0.72))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_1()
                    .border_color(hsla(0.0, 0.0, 1.0, 0.24))
                    .bg(linear_gradient(
                        145.0,
                        linear_color_stop(hsla(0.1, 0.025, 0.42, 1.0), 0.0),
                        linear_color_stop(hsla(0.1, 0.025, 0.22, 1.0), 1.0),
                    ))
                    .child(
                        div()
                            .size(px(tonearm_base_size * 0.28))
                            .rounded_full()
                            .border_1()
                            .border_color(hsla(0.0, 0.0, 1.0, 0.2))
                            .bg(linear_gradient(
                                135.0,
                                linear_color_stop(hsla(0.1, 0.025, 0.82, 1.0), 0.0),
                                linear_color_stop(hsla(0.1, 0.025, 0.56, 1.0), 1.0),
                            )),
                    ),
            );
        let highlight = Icon::new(AppIcon::VinylHighlight)
            .absolute()
            .top_0()
            .left_0()
            .with_size(px(size))
            .text_color(hsla(0.0, 0.0, 1.0, 0.2))
            .transform(Transformation::rotate(percentage(rotation_phase)));

        // GPUI caches the SVG mask at this size; denser grooves still use one sprite.
        let grooves = Icon::new(AppIcon::VinylGrooves)
            .absolute()
            .top_0()
            .left_0()
            .with_size(px(size))
            .text_color(hsla(0.0, 0.0, 1.0, 0.12));

        div()
            .relative()
            .w(px(size))
            .h(px(size))
            .flex_none()
            .rounded_full()
            .bg(linear_gradient(
                132.0,
                linear_color_stop(hsla(0.0, 0.0, 0.115, 1.0), 0.0),
                linear_color_stop(hsla(0.0, 0.0, 0.025, 1.0), 1.0),
            ))
            .shadow(vec![
                box_shadow(
                    px(0.0),
                    px(size * 0.008),
                    px(size * 0.015),
                    px(0.0),
                    hsla(0.0, 0.0, 0.0, 0.32),
                ),
                box_shadow(
                    px(0.0),
                    px(size * 0.035),
                    px(size * 0.08),
                    px(-size * 0.015),
                    hsla(0.0, 0.0, 0.0, 0.2),
                ),
            ])
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_full()
                    .border_1()
                    .border_color(hsla(0.0, 0.0, 0.0, 0.5)),
            )
            .child(
                div()
                    .absolute()
                    .inset(px(size * 0.006))
                    .rounded_full()
                    .border_1()
                    .border_color(hsla(0.0, 0.0, 1.0, 0.09)),
            )
            .child(highlight)
            .child(grooves)
            .child(
                div()
                    .absolute()
                    .top(px(label_offset))
                    .left(px(label_offset))
                    .size(px(label_size))
                    .rounded_full()
                    .border_1()
                    .border_color(hsla(0.0, 0.0, 1.0, 0.14))
                    .bg(linear_gradient(
                        132.0,
                        linear_color_stop(
                            hsla(0.0, 0.0, 0.12, 1.0).blend(metal_tint.opacity(0.12)),
                            0.0,
                        ),
                        linear_color_stop(hsla(0.0, 0.0, 0.035, 1.0), 1.0),
                    ))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        self.vinyl_cover
                            .render(inner_cover_size, rotation_phase)
                            .unwrap_or_else(|| {
                                img(cover)
                                    .size(px(inner_cover_size))
                                    .aspect_square()
                                    .object_fit(ObjectFit::Cover)
                                    .flex_none()
                                    .rounded_full()
                                    .into_any_element()
                            }),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px((label_size - hole_size) * 0.5))
                            .left(px((label_size - hole_size) * 0.5))
                            .size(px(hole_size))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(linear_gradient(
                                132.0,
                                linear_color_stop(hsla(0.0, 0.0, 0.62, 1.0), 0.0),
                                linear_color_stop(hsla(0.0, 0.0, 0.14, 1.0), 1.0),
                            ))
                            .border_1()
                            .border_color(hsla(0.0, 0.0, 0.0, 0.65))
                            .child(
                                div()
                                    .size(px(size * 0.012))
                                    .rounded_full()
                                    .bg(hsla(0.0, 0.0, 0.025, 1.0)),
                            ),
                    ),
            )
            .child(tonearm_layer(
                AppIcon::Tonearm,
                "shadow-close",
                size * 0.005,
                size * 0.008,
                hsla(0.0, 0.0, 0.0, 0.24),
            ))
            .child(tonearm_layer(
                AppIcon::Tonearm,
                "body",
                0.0,
                0.0,
                metal_body,
            ))
            .child(tonearm_layer(
                AppIcon::TonearmShade,
                "shade",
                0.0,
                0.0,
                hsla(0.1, 0.025, 0.12, 0.94),
            ))
            .child(tonearm_layer(
                AppIcon::TonearmHighlight,
                "highlight",
                0.0,
                0.0,
                hsla(0.0, 0.0, 1.0, 0.5),
            ))
            .child(tonearm_layer(
                AppIcon::TonearmStylus,
                "stylus-shadow",
                size * 0.005,
                size * 0.008,
                hsla(0.0, 0.0, 0.0, 0.24),
            ))
            .child(tonearm_layer(
                AppIcon::TonearmStylus,
                "stylus",
                0.0,
                0.0,
                hsla(0.0, 0.0, 1.0, 0.65),
            ))
            .child(tonearm_base)
            .into_any_element()
    }

    fn page_header(
        &self,
        title: impl Into<SharedString>,
        subtitle: impl Into<SharedString>,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        v_flex()
            .gap_1()
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title.into()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(subtitle.into()),
            )
            .into_any_element()
    }

    fn render_scrolling_title(
        &self,
        text: SharedString,
        viewport_width: f32,
        id: SharedString,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        self.render_scrolling_text(
            text,
            viewport_width,
            rems(0.875).to_pixels(window.rem_size()),
            FontWeight::MEDIUM,
            id,
            window,
        )
    }

    fn render_scrolling_text(
        &self,
        text: SharedString,
        viewport_width: f32,
        font_size: Pixels,
        weight: FontWeight,
        id: SharedString,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        let text_style = window.text_style().highlight(weight);
        let text_width = {
            let cache = self.title_width_cache.borrow();
            let key = (f32::from(font_size).to_bits(), weight, text.clone());
            if let Some(&width) = cache.get(&key) {
                width
            } else {
                drop(cache);
                let width = window
                    .text_system()
                    .shape_line(
                        text.clone(),
                        font_size,
                        &[text_style.to_run(text.len())],
                        None,
                    )
                    .width;
                let mut cache = self.title_width_cache.borrow_mut();
                if cache.len() > 20_000 {
                    cache.clear();
                }
                cache.insert(key, width);
                width
            }
        };
        let viewport_width = px(viewport_width);

        if text_width <= viewport_width {
            return div()
                .w(viewport_width)
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(font_size)
                .font_weight(weight)
                .child(text)
                .into_any_element();
        }

        let gap = px(32.0);
        let travel = text_width + gap;
        let travel_px = f32::from(travel);
        let duration_seconds = (travel_px / 28.0_f32 + 1.5_f32).max(4.0_f32);
        let hold_fraction = (1.5_f32 / duration_seconds).clamp(0.0_f32, 0.8_f32);
        let animation = Animation::new(Duration::from_secs_f32(duration_seconds))
            .repeat()
            .with_easing(move |delta| {
                if delta <= hold_fraction {
                    0.0
                } else {
                    (delta - hold_fraction) / (1.0 - hold_fraction)
                }
            });
        let repeated_text = text.clone();

        div()
            .w(viewport_width)
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .child(
                h_flex()
                    .flex_none()
                    .gap(gap)
                    .text_size(font_size)
                    .font_weight(weight)
                    .child(text)
                    .child(repeated_text)
                    .with_animation(id, animation, move |this, delta| {
                        this.ml(px(-travel_px * delta))
                    }),
            )
            .into_any_element()
    }

    fn render_album_grid(
        &self,
        scope: &str,
        albums: &[Album],
        cx: &Context<Self>,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        let grid_start = std::time::Instant::now();
        let grid =
            h_flex()
                .items_start()
                .flex_wrap()
                .gap_5()
                .children(albums.iter().enumerate().map(|(index, album)| {
                    let key = item_instance_key(scope, "album", index, &album.id);
                    let album_for_click = album.clone();
                    let album_for_play = album.clone();
                    let album_for_cover = album.clone();
                    let album_id_for_hover = key.clone();
                    v_flex()
                        .id(SharedString::from(key.clone()))
                        .w(px(176.0))
                        .gap_2()
                        .child(
                            div()
                                .id(SharedString::from(format!("album-cover-{}", album.id)))
                                .relative()
                                .w(px(176.0))
                                .h(px(176.0))
                                .cursor_pointer()
                                .on_hover(cx.listener(move |this, hovering, _, cx| {
                                    this.set_hovered(&album_id_for_hover, *hovering, cx);
                                }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_album(album_for_cover.clone());
                                    cx.notify();
                                }))
                                .child(self.render_cover(album.cover_art.as_deref(), 176.0, cx))
                                .child(div().absolute().top_2().right_2().child(
                                    self.favorite_button(FavoriteKind::Album, &album.id, cx),
                                ))
                                .when(
                                    self.state.hovered_item.as_deref() == Some(key.as_str()),
                                    |this| {
                                        this.child(
                                            div()
                                                .absolute()
                                                .inset_0()
                                                .rounded_lg()
                                                .bg(hsla(0.0, 0.0, 0.0, 0.32))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                    cx.stop_propagation()
                                                })
                                                .child(
                                                    div()
                                                        .id(SharedString::from(format!(
                                                            "album-play-{}",
                                                            album.id
                                                        )))
                                                        .size(px(48.0))
                                                        .rounded_full()
                                                        .bg(hsla(0.0, 0.0, 1.0, 0.92))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .shadow_md()
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.play_album(
                                                                    album_for_play.clone(),
                                                                );
                                                                cx.notify();
                                                            },
                                                        ))
                                                        .child(
                                                            Icon::new(PlayerIcon::Play)
                                                                .size(px(22.0))
                                                                .text_color(hsla(
                                                                    0.0, 0.0, 0.12, 1.0,
                                                                )),
                                                        ),
                                                ),
                                        )
                                    },
                                ),
                        )
                        .child(
                            Button::new(SharedString::from(format!("album-{}", album.id)))
                                .ghost()
                                .w_full()
                                .justify_start()
                                .child(self.render_scrolling_title(
                                    album.name.clone().into(),
                                    144.0,
                                    SharedString::from(format!("album-title-{}", album.id)),
                                    window,
                                ))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_album(album_for_click.clone());
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .px_2()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(format!(
                                    "{}{}",
                                    album.artist,
                                    album
                                        .year
                                        .map(|year| format!(" - {year}"))
                                        .unwrap_or_default()
                                )),
                        )
                }));
        let grid_elapsed = grid_start.elapsed();
        if grid_elapsed > Duration::from_millis(8) {
            log::warn!(
                "render_album_grid built {} albums in {grid_elapsed:?}",
                albums.len()
            );
        }
        grid.into_any_element()
    }

    /// 专辑网格骨架屏：数据加载期间占位，避免空页面跳动。
    fn render_album_skeleton_grid(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let muted = cx.theme().muted_foreground;
        h_flex()
            .items_start()
            .flex_wrap()
            .gap_5()
            .children((0..12).map(|_| {
                v_flex()
                    .w(px(176.0))
                    .gap_2()
                    .child(
                        div()
                            .w(px(176.0))
                            .h(px(176.0))
                            .rounded_md()
                            .bg(muted.opacity(0.08)),
                    )
                    .child(
                        div()
                            .w(px(128.0))
                            .h(px(12.0))
                            .rounded_full()
                            .bg(muted.opacity(0.1)),
                    )
                    .child(
                        div()
                            .w(px(88.0))
                            .h(px(10.0))
                            .rounded_full()
                            .bg(muted.opacity(0.07)),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }

    /// 列表行骨架屏：播放列表 / 歌曲列表加载期间占位。
    fn render_list_skeleton(&self, rows: usize, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let muted = cx.theme().muted_foreground;
        v_flex()
            .gap_2()
            .children((0..rows).map(|_| {
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .w(px(40.0))
                            .h(px(40.0))
                            .rounded_md()
                            .bg(muted.opacity(0.08)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_1()
                            .child(
                                div()
                                    .h(px(12.0))
                                    .w_full()
                                    .max_w(px(240.0))
                                    .rounded_full()
                                    .bg(muted.opacity(0.1)),
                            )
                            .child(
                                div()
                                    .h(px(10.0))
                                    .w(px(120.0))
                                    .rounded_full()
                                    .bg(muted.opacity(0.07)),
                            ),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }

    /// 艺术家封面网格骨架屏：与 artist 页的圆角方形封面一致。
    fn render_artist_skeleton_grid(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let muted = cx.theme().muted_foreground;
        h_flex()
            .items_start()
            .flex_wrap()
            .gap_5()
            .children((0..15).map(|_| {
                v_flex()
                    .w(px(152.0))
                    .gap_2()
                    .child(
                        div()
                            .w(px(152.0))
                            .h(px(152.0))
                            .rounded_lg()
                            .bg(muted.opacity(0.08)),
                    )
                    .child(
                        div()
                            .w(px(104.0))
                            .h(px(12.0))
                            .rounded_full()
                            .bg(muted.opacity(0.1)),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }

    fn render_artist_grid(
        &self,
        artists: &[Artist],
        cx: &Context<Self>,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        h_flex()
            .items_start()
            .flex_wrap()
            .gap_5()
            .children(artists.iter().enumerate().map(|(index, artist)| {
                let key = item_instance_key("artists", "artist", index, &artist.id);
                let artist_for_click = artist.clone();
                let artist_for_play = artist.clone();
                let artist_for_cover = artist.clone();
                let artist_id_for_hover = key.clone();
                v_flex()
                    .id(SharedString::from(key.clone()))
                    .w(px(152.0))
                    .gap_2()
                    .child(
                        div()
                            .id(SharedString::from(format!("artist-cover-{}", artist.id)))
                            .relative()
                            .w(px(152.0))
                            .h(px(152.0))
                            .cursor_pointer()
                            .on_hover(cx.listener(move |this, hovering, _, cx| {
                                this.set_hovered(&artist_id_for_hover, *hovering, cx);
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_artist(artist_for_cover.clone());
                                cx.notify();
                            }))
                            .child(self.render_cover(artist.cover_art.as_deref(), 152.0, cx))
                            .child(
                                div()
                                    .absolute()
                                    .top_2()
                                    .right_2()
                                    .child(self.favorite_button(
                                        FavoriteKind::Artist,
                                        &artist.id,
                                        cx,
                                    )),
                            )
                            .when(
                                self.state.hovered_item.as_deref() == Some(key.as_str()),
                                |this| {
                                    this.child(
                                        div()
                                            .absolute()
                                            .inset_0()
                                            .rounded_lg()
                                            .bg(hsla(0.0, 0.0, 0.0, 0.32))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                cx.stop_propagation()
                                            })
                                            .child(
                                                div()
                                                    .id(SharedString::from(format!(
                                                        "artist-play-{}",
                                                        artist.id
                                                    )))
                                                    .size(px(48.0))
                                                    .rounded_full()
                                                    .bg(hsla(0.0, 0.0, 1.0, 0.92))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .shadow_md()
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.play_artist(artist_for_play.clone());
                                                        cx.notify();
                                                    }))
                                                    .child(
                                                        Icon::new(PlayerIcon::Play)
                                                            .size(px(22.0))
                                                            .text_color(hsla(0.0, 0.0, 0.12, 1.0)),
                                                    ),
                                            ),
                                    )
                                },
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("artist-{}", artist.id)))
                            .ghost()
                            .w_full()
                            .justify_start()
                            .child(self.render_scrolling_title(
                                artist.name.clone().into(),
                                120.0,
                                SharedString::from(format!("artist-title-{}", artist.id)),
                                window,
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_artist(artist_for_click.clone());
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .px_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                artist
                                    .album_count
                                    .map(|count| format!("{count} albums"))
                                    .unwrap_or_else(|| "Artist".to_string()),
                            ),
                    )
            }))
            .into_any_element()
    }

    fn render_playing_indicator(
        &self,
        song_id: &str,
        current: bool,
        animated: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        if !current {
            if self.state.hovered_item.as_deref() == Some(song_id) {
                return Icon::new(PlayerIcon::Play)
                    .size(px(15.0))
                    .text_color(cx.theme().muted_foreground)
                    .into_any_element();
            }
            return div().w(px(16.0)).h(px(16.0)).flex_none().into_any_element();
        }

        let bar_heights = [7.0_f32, 13.0_f32, 9.0_f32];
        h_flex()
            .w(px(16.0))
            .h(px(16.0))
            .flex_none()
            .items_end()
            .justify_center()
            .gap(px(2.0))
            .children(bar_heights.into_iter().enumerate().map(|(index, height)| {
                let bar = div()
                    .w(px(3.0))
                    .h(px(height))
                    .rounded_full()
                    .bg(cx.theme().info);

                if animated {
                    let phase = index as f32 * 0.23;
                    bar.with_animation(
                        SharedString::from(format!("playing-{song_id}-{index}")),
                        Animation::new(Duration::from_millis(720))
                            .repeat()
                            .with_easing(move |delta| {
                                let shifted = (delta + phase) % 1.0;
                                1.0 - (shifted * 2.0 - 1.0).abs()
                            }),
                        |bar, delta| bar.h(px(4.0 + delta * 10.0)),
                    )
                    .into_any_element()
                } else {
                    bar.opacity(0.7).into_any_element()
                }
            }))
            .into_any_element()
    }

    fn render_song_list(
        &self,
        scope: &'static str,
        editable: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        // uniform_list 视口虚拟化：只构建可见行，长列表不再一次性挂载全部元素，
        // 滚动多久元素树就只有多大，与列表总长度无关。
        let Some(songs) = self.songs_for_scope(scope) else {
            return div().into_any_element();
        };
        if songs.is_empty() {
            return div().into_any_element();
        }
        let handle = self.song_list_scroll(scope);
        // 专辑 / 播放列表详情和队列页中表格占满剩余高度（表头固定，列表内部滚动）；
        // 收藏与搜索页的歌曲表格与其他分区共存，限制高度独立滚动。
        let fill = matches!(scope, "album-detail" | "playlist-detail" | "queue");
        let rows = cx.processor(
            move |this, range: Range<usize>, _window, cx| {
                let playback = this.audio.state();
                range
                    .map(|index| this.render_song_row(scope, index, editable, &playback, cx))
                    .collect::<Vec<_>>()
            },
        );
        // 限高模式：显式给出列表高度 min(行数×行高, 视口)，不依赖 max_h。
        // taffy 计算自动高度父容器的内容尺寸时会忽略 max 约束（ContentSize
        // 模式不套用 min/max），Infer+max_h 会让列表按全部内容高度参与布局。
        let viewport = self.content_scroll_handle.bounds().size.height;
        let cap = if viewport > px(0.0) {
            (viewport - px(36.0)).max(px(300.0))
        } else {
            px(600.0)
        };
        let capped_height = px((songs.len() as f32 * SONG_ROW_HEIGHT).min(f32::from(cap)));
        let list = uniform_list(
            SharedString::from(format!("song-list-{scope}")),
            songs.len(),
            rows,
        )
        .track_scroll(&handle)
        .on_scroll_wheel(cx.listener(|this, _: &ScrollWheelEvent, _, _| {
            this.maybe_load_visible_song_covers();
        }))
        .when(fill, |list| list.flex_1().min_h_0())
        .when(!fill, |list| list.h(capped_height));
        v_flex()
            .relative()
            .w_full()
            .border_1()
            .border_color(cx.theme().border)
            .rounded_lg()
            .overflow_hidden()
            .when(fill, |this| this.flex_1().min_h_0())
            .child(
                h_flex()
                    .h(px(36.0))
                    .flex_none()
                    .px_3()
                    .gap_3()
                    .bg(cx.theme().secondary)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().w(px(34.0)))
                    .child(div().w(px(42.0)))
                    .child(div().flex_1().flex_grow(3.0).child("Title"))
                    .child(
                        div()
                            .flex_1()
                            .flex_grow(2.0)
                            .max_w(px(180.0))
                            .child("Artist"),
                    )
                    .child(div().w(px(64.0)).text_right().child("Time"))
                    .child(div().w(px(if editable { 124.0 } else { 92.0 }))),
            )
            .child(list)
            .child(Scrollbar::vertical(&handle))
            .into_any_element()
    }

    /// 渲染歌曲列表中的一行。行内事件只捕获 (scope, index)，
    /// 触发时再从当前状态解析列表与歌曲，避免每次渲染克隆整个列表。
    fn render_song_row(
        &self,
        scope: &'static str,
        index: usize,
        editable: bool,
        playback: &PlaybackState,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(song) = self.song_for_scope(scope, index) else {
            return div().into_any_element();
        };
        let total = self.songs_for_scope(scope).map_or(0, <[Song]>::len);
        let key = item_instance_key(scope, "song", index, &song.id);
        let menu_app = cx.entity().downgrade();
        let button_menu_app = menu_app.clone();
        let current = self
            .state
            .now_playing
            .as_ref()
            .is_some_and(|playing| playing.id == song.id)
            && (!editable || self.state.queue_index == Some(index));
        let animated = current && playback.active && !playback.paused;
        let album = if song.album.trim().is_empty() {
            "Unknown album".to_string()
        } else {
            song.album.clone()
        };
        // uniform_list 把每行当作独立根元素布局（Definite 可用宽度），
        // 不会像普通容器那样横向拉伸，必须显式 w_full：否则行宽收缩为
        // 内容宽度，行尾按钮、悬停/正在播放高亮都无法纵向对齐。
        let row = h_flex()
            .id(SharedString::from(key.clone()))
            .w_full()
            .h(px(60.0))
            .px_3()
            .gap_3()
            .relative()
            .border_t_1()
            .border_color(cx.theme().border.opacity(0.6))
            .hover(|style| style.bg(cx.theme().accent.opacity(0.12)))
            .cursor_pointer();
        let row = if current {
            row.bg(cx.theme().info.opacity(0.1))
        } else {
            row
        };
        row.child(
            div()
                .w(px(34.0))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .child(self.favorite_button(FavoriteKind::Song, &song.id, cx)),
        )
            .child(
                div()
                    .w(px(42.0))
                    .h(px(42.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border.opacity(0.7))
                    .bg(cx.theme().background)
                    .child(self.render_cover(song.cover_art.as_deref(), 36.0, cx)),
            )
            .child(
                h_flex()
                    .flex_1()
                    .flex_grow(3.0)
                    .min_w_0()
                    .gap_2()
                    .text_color(if current {
                        cx.theme().info
                    } else {
                        cx.theme().foreground
                    })
                    .child(self.render_playing_indicator(&key, current, animated, cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .line_height(rems(1.25))
                                    .truncate()
                                    .child(song.title.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .line_height(rems(1.2))
                                    .truncate()
                                    .child(album),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex_grow(3.0)
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .truncate()
                    .child(song.artist.clone()),
            )
            .child(
                div()
                    .w(px(64.0))
                    .text_sm()
                    .text_right()
                    .text_color(cx.theme().muted_foreground)
                    .child(format_duration(song.duration)),
            )
            .child(
                // 行尾操作列：固定宽度、常规布局，不再用悬浮层盖住行内容。
                h_flex()
                    .flex_none()
                    .h_full()
                    .w(px(if editable { 124.0 } else { 92.0 }))
                    .items_center()
                    .justify_end()
                    .gap_0p5()
                    .when(editable, |slot| {
                        slot.child(
                            Button::new(SharedString::from(format!("queue-up-{key}")))
                                .icon(AppIcon::ChevronUp)
                                .tooltip("Move up")
                                .ghost()
                                .small()
                                .disabled(index == 0)
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation()
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.move_queue_item(index, index - 1);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("queue-down-{key}")))
                                .icon(AppIcon::ChevronDown)
                                .tooltip("Move down")
                                .ghost()
                                .small()
                                .disabled(index + 1 >= total)
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation()
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.move_queue_item(index, index + 1);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("queue-remove-{key}")))
                                .icon(AppIcon::Close)
                                .tooltip("Remove from queue")
                                .ghost()
                                .small()
                                .text_color(cx.theme().muted_foreground)
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation()
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_from_queue(index);
                                    cx.notify();
                                })),
                        )
                    })
                    .when(!editable, |slot| {
                        slot.child(
                            Button::new(SharedString::from(format!("play-next-{key}")))
                                .icon(PlayerIcon::Next)
                                .tooltip("Play next")
                                .ghost()
                                .small()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation()
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(song) = this.song_for_scope(scope, index) {
                                        this.insert_next(song.clone());
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("add-to-queue-{key}")))
                                .icon(AppIcon::Queue)
                                .tooltip("Add to queue")
                                .ghost()
                                .small()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation()
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(song) = this.song_for_scope(scope, index) {
                                        this.add_to_queue(song.clone());
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        div()
                            .id("song-actions-trigger")
                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation()
                            })
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(
                                Button::new(SharedString::from(format!("song-menu-{key}")))
                                    .icon(IconName::Ellipsis)
                                    .tooltip("Song actions")
                                    .ghost()
                                    .small()
                                    .dropdown_menu(move |menu, _, cx| {
                                        button_menu_app
                                            .update(cx, |this, cx| {
                                                let Some(context) = this
                                                    .song_menu_context(scope, index, editable)
                                                else {
                                                    return menu;
                                                };
                                                this.song_menu(menu, context, cx)
                                            })
                                            .expect("app owns song menu")
                                    }),
                            ),
                    ),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                // 单击立即播放该歌曲
                this.play_song_at_scope(scope, index);
                cx.notify();
            }))
            .context_menu(move |menu, _, cx| {
                menu_app
                    .update(cx, |this, cx| {
                        let context = this.song_menu_context(scope, index, editable);
                        let Some(context) = context else {
                            return menu;
                        };
                        this.song_menu(menu, context, cx)
                    })
                    .unwrap_or_else(|_| unreachable!("app outlives its rows"))
            })
            .into_any_element()
    }

    /// 打开歌曲菜单时从当前状态构建菜单上下文；列表已变化时返回 None。
    fn song_menu_context(
        &self,
        scope: &str,
        index: usize,
        editable: bool,
    ) -> Option<SongMenuContext> {
        let songs = self.songs_for_scope(scope)?;
        if index >= songs.len() {
            return None;
        }
        Some(SongMenuContext {
            songs: Arc::new(songs.to_vec()),
            index,
            queue: editable,
            playlist: (scope == "playlist-detail")
                .then(|| self.state.current_playlist.clone())
                .flatten(),
        })
    }
    fn render_server_status_card(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let status_label = if self.state.loading {
            "Connecting..."
        } else if self.state.server.is_some() {
            "Connected"
        } else {
            "Not connected"
        };
        let status_color = if self.state.error.is_some() {
            cx.theme().red
        } else if self.state.server.is_some() {
            cx.theme().accent
        } else {
            cx.theme().muted_foreground
        };
        let server_name = self
            .state
            .server
            .as_ref()
            .map(|server| {
                format!(
                    "{} {}",
                    server.app_name.as_deref().unwrap_or("Navidrome"),
                    server.version
                )
            })
            .unwrap_or_else(|| "Server entry".to_string());
        let server_url = if self.config.server_url.trim().is_empty() {
            "No server URL configured".to_string()
        } else {
            self.config.server_url.clone()
        };
        let username = if self.config.username.trim().is_empty() {
            "No username configured".to_string()
        } else {
            self.config.username.clone()
        };

        v_flex()
            .w_full()
            .gap_4()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary.opacity(0.35))
            .child(
                h_flex()
                    .justify_between()
                    .items_start()
                    .gap_4()
                    .child(
                        v_flex()
                            .min_w_0()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Primary server"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(server_name),
                            )
                            .child(div().text_xs().text_color(status_color).child(status_label)),
                    )
                    .when(self.api.is_some(), |this| {
                        this.child(
                            Button::new("settings-refresh")
                                .icon(AppIcon::Refresh)
                                .tooltip("Refresh library")
                                .loading(self.state.loading)
                                .ghost()
                                .small()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.refresh_library();
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_6()
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Server URL"),
                            )
                            .child(div().text_sm().truncate().child(server_url)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Username"),
                            )
                            .child(div().text_sm().truncate().child(username)),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.state.status.clone()),
            )
            .when_some(self.state.error.as_ref(), |this, error| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().red)
                        .child(error.clone()),
                )
            })
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Server URL"),
                    )
                    .child(
                        div()
                            .w_full()
                            .on_key_down(|_, _, cx| cx.stop_propagation())
                            .child(Input::new(&self.server_input).w_full()),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Username"),
                    )
                    .child(
                        div()
                            .w_full()
                            .on_key_down(|_, _, cx| cx.stop_propagation())
                            .child(Input::new(&self.username_input).w_full()),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Password"),
                    )
                    .child(
                        div()
                            .w_full()
                            .on_key_down(|_, _, cx| cx.stop_propagation())
                            .child(Input::new(&self.password_input).w_full()),
                    ),
            )
            .child(
                h_flex().justify_end().gap_2().child(
                    Button::new("save-settings")
                        .label("Save and connect")
                        .primary()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.save_settings(cx);
                            cx.notify();
                        })),
                ),
            )
            .into_any_element()
    }

    fn render_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let cache_dir = config::audio_cache_dir(&self.config);
        v_flex()
            .w_full()
            .max_w(px(840.0))
            .gap_6()
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Servers"),
                    )
                    .child(self.render_server_status_card(cx)),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Playback"),
                    )
                    .child(
                        v_flex()
                            .gap_4()
                            .p_4()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().secondary.opacity(0.2))
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Audio cache folder"),
                                    )
                                    .child(
                                        h_flex()
                                            .w_full()
                                            .min_w_0()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .truncate()
                                                    .text_sm()
                                                    .child(cache_dir.display().to_string()),
                                            )
                                            .child(
                                                Button::new("choose-cache-directory")
                                                    .label("Choose folder")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.choose_cache_directory(cx);
                                                    })),
                                            )
                                            .child(
                                                Button::new("reset-cache-directory")
                                                    .label("Use default")
                                                    .disabled(self.config.cache_dir.is_none())
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.apply_cache_directory(None);
                                                        cx.notify();
                                                    })),
                                            ),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Streaming quality"),
                                    )
                                    .child(h_flex().gap_2().flex_wrap().children(
                                        TranscodingQuality::ALL.into_iter().map(|quality| {
                                            Button::new(SharedString::from(format!(
                                                "quality-{}",
                                                quality.cache_profile()
                                            )))
                                            .label(quality.label())
                                            .selected(self.config.transcoding_quality == quality)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.set_transcoding_quality(quality, cx);
                                            }))
                                        }),
                                    )),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Volume normalization"),
                    )
                    .child(
                        v_flex()
                            .gap_4()
                            .p_4()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().secondary.opacity(0.2))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        "Adjust per-song loudness with ReplayGain so quiet and loud tracks play at a similar level.",
                                    ),
                            )
                            .child(
                                h_flex().gap_2().flex_wrap().children(
                                    VolumeNormalization::ALL.into_iter().map(|mode| {
                                        Button::new(SharedString::from(format!(
                                            "volume-normalization-{}",
                                            mode.label().to_ascii_lowercase()
                                        )))
                                        .label(mode.label())
                                        .tooltip(mode.tooltip())
                                        .selected(self.config.volume_normalization == mode)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.set_volume_normalization(mode, cx);
                                        }))
                                    }),
                                ),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Appearance"),
                    )
                    .child(
                        v_flex()
                            .gap_3()
                            .p_4()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().secondary.opacity(0.2))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Theme"),
                            )
                            .child(
                                h_flex().gap_2().children(
                                    [
                                        ThemePreference::Light,
                                        ThemePreference::Dark,
                                        ThemePreference::System,
                                    ]
                                    .into_iter()
                                    .map(|preference| {
                                        Button::new(SharedString::from(format!(
                                            "theme-{}",
                                            preference.label().to_lowercase()
                                        )))
                                        .label(preference.label())
                                        .selected(self.config.theme == preference)
                                        .on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                this.set_theme(preference, window, cx);
                                            }),
                                        )
                                    }),
                                ),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn error_banner(&self, cx: &Context<Self>) -> Option<gpui_kit::AnyElement> {
        self.state.error.as_ref().map(|message| {
            div()
                .p_3()
                .rounded_md()
                .bg(cx.theme().red.opacity(0.12))
                .text_color(cx.theme().red)
                .child(message.clone())
                .into_any_element()
        })
    }

    fn render_now_playing(&self, window: &Window, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let Some(song) = &self.state.now_playing else {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    div()
                        .text_xl()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("No track playing"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Choose a song from your library."),
                )
                .into_any_element();
        };

        let viewport_width = f32::from(window.viewport_size().width);
        let viewport_height = f32::from(window.viewport_size().height);
        let max_cover_size = if viewport_width < 1_100.0 {
            300.0
        } else {
            380.0
        };
        let cover_size = (viewport_height - 300.0).clamp(220.0, max_cover_size);
        let playback = self.audio.state();
        let accent = self.now_playing_accent(cx);
        let active_line = self
            .state
            .lyrics
            .as_ref()
            .and_then(|lyrics| lyrics.active_line_index(playback.position));
        let lyrics = self.state.lyrics.as_ref();
        let lyrics_body = if self.state.lyrics_loading {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child("Loading lyrics...")
                .into_any_element()
        } else if let Some(error) = &self.state.lyrics_error {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(cx.theme().muted_foreground)
                .child("Lyrics are unavailable for this track.")
                .child(div().text_xs().child(error.clone()))
                .into_any_element()
        } else if let Some(lyrics) = lyrics.filter(|lyrics| !lyrics.lines.is_empty()) {
            let synced = lyrics.is_synced();
            v_flex()
                .id("lyrics-scroll")
                .flex_1()
                .min_h_0()
                .px_6()
                .overflow_y_scroll()
                .track_scroll(&self.lyrics_scroll_handle)
                .child(div().h(px(240.0)).flex_none())
                .children(lyrics.lines.iter().enumerate().map(|(index, lyric_line)| {
                    let current = synced && active_line == Some(index);
                    let distance = active_line
                        .map(|active| active.abs_diff(index))
                        .unwrap_or(usize::MAX);
                    let text = if lyric_line.text.is_empty() {
                        " ".to_string()
                    } else {
                        lyric_line.text.clone()
                    };
                    let start_ms = lyric_line.start_ms;
                    let line = div()
                        .id(SharedString::from(format!(
                            "lyric-line-{}-{index}",
                            song.id
                        )))
                        .w_full()
                        .max_w(px(760.0))
                        .min_h(px(64.0))
                        .mx_auto()
                        .px_4()
                        .py_2()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_center()
                        .line_height(rems(1.45))
                        .when_some(start_ms, |this, start_ms| {
                            this.cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.seek_playback(Duration::from_millis(start_ms));
                                    cx.notify();
                                }))
                        })
                        .child(text);

                    if current {
                        line.text_2xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(accent)
                            .with_animation(
                                SharedString::from(format!("active-lyric-{}-{index}", song.id)),
                                Animation::new(Duration::from_millis(220))
                                    .with_easing(ease_out_quint()),
                                |this, delta| this.opacity(0.64 + delta * 0.36),
                            )
                            .into_any_element()
                    } else if synced {
                        let opacity = match distance {
                            1 => 0.68,
                            2 => 0.52,
                            3 => 0.4,
                            _ => 0.28,
                        };
                        line.text_lg()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().foreground.opacity(opacity))
                            .into_any_element()
                    } else {
                        line.text_lg()
                            .text_color(cx.theme().foreground.opacity(0.72))
                            .into_any_element()
                    }
                }))
                .child(div().h(px(280.0)).flex_none())
                .into_any_element()
        } else {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child("No lyrics are available for this track.")
                .into_any_element()
        };

        let rotation_phase = self.vinyl_cover.phase();
        h_flex()
            .size_full()
            .min_h_0()
            .px_8()
            .py_5()
            .gap_8()
            .items_center()
            .child(
                v_flex()
                    .w(px((cover_size + 40.0).max(320.0)))
                    .flex_none()
                    .items_center()
                    .gap_4()
                    .child(self.render_vinyl_record(
                        song.cover_art.as_deref(),
                        cover_size,
                        rotation_phase,
                        playback.active && !playback.paused,
                        cx,
                    ))
                    .child(
                        v_flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(song.title.clone()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(song.artist.clone()),
                            )
                            .when(!song.album.is_empty(), |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(song.album.clone()),
                                )
                            }),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .pl_6()
                    .child(
                        v_flex()
                            .items_center()
                            .gap_1()
                            .pb_1()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Lyrics"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground.opacity(0.82))
                                    .child(format!("{} - {}", song.title, song.artist)),
                            ),
                    )
                    .child(lyrics_body),
            )
            .into_any_element()
    }

    /// Home 页可折叠区块：标题行（点击折叠/展开）+ 内容。后续新增区块只需传入唯一 id 与内容。
    fn render_home_section(
        &self,
        id: &'static str,
        title: &str,
        content: gpui_kit::AnyElement,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let collapsed = self.state.collapsed_sections.contains(id);
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .cursor_pointer()
                    .id(SharedString::from(format!("home-section-{id}")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_home_section(id, cx);
                    }))
                    .child(
                        Icon::new(if collapsed {
                            AppIcon::ChevronRight
                        } else {
                            AppIcon::ChevronDown
                        })
                        .size(px(16.0))
                        .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title.to_string()),
                    ),
            )
            .when(!collapsed, |this| this.child(content))
            .into_any_element()
    }

    /// Home 页专辑区块的通用内容：加载骨架 / 空状态提示 / 专辑网格。
    fn render_home_album_content(
        &self,
        scope: &str,
        albums: &[Album],
        loading: bool,
        empty_text: &str,
        cx: &Context<Self>,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        if albums.is_empty() && loading {
            self.render_album_skeleton_grid(cx)
        } else if albums.is_empty() {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(empty_text.to_string())
                .into_any_element()
        } else {
            self.render_album_grid(scope, albums, cx, window)
        }
    }

    fn toggle_home_section(&mut self, id: &'static str, cx: &mut Context<Self>) {
        if self.state.collapsed_sections.contains(id) {
            self.state.collapsed_sections.remove(id);
        } else {
            self.state.collapsed_sections.insert(id.to_string());
        }
        cx.notify();
    }

    fn render_content(&self, window: &mut Window, cx: &Context<Self>) -> gpui_kit::AnyElement {
        if self.state.settings_open {
            return self.render_settings(cx);
        }

        match self.state.view {
            View::Home => v_flex()
                .gap_5()
                .child(
                    self.page_header(
                        "Home",
                        self.state
                            .server
                            .as_ref()
                            .map(|info| format!("Connected to Navidrome API {}", info.version))
                            .unwrap_or_else(|| self.state.status.clone()),
                        cx,
                    ),
                )
                .children(self.error_banner(cx))
                .child(self.render_home_section(
                    "recent",
                    "Recently played",
                    self.render_home_album_content(
                        "recent",
                        &self.state.recent_albums,
                        self.state.recent_albums_loading,
                        "Nothing played yet — recently played albums will appear here.",
                        cx,
                        window,
                    ),
                    cx,
                ))
                .child(self.render_home_section(
                    "frequent",
                    "Frequently played",
                    self.render_home_album_content(
                        "frequent",
                        &self.state.frequent_albums,
                        self.state.frequent_albums_loading,
                        "Nothing frequently played yet — your most-played albums will appear here.",
                        cx,
                        window,
                    ),
                    cx,
                ))
                .child(self.render_home_section(
                    "random",
                    "Random albums",
                    self.render_home_album_content(
                        "random",
                        &self.state.random_albums,
                        self.state.random_albums_loading,
                        "No random albums to show — refresh the library for a fresh pick.",
                        cx,
                        window,
                    ),
                    cx,
                ))
                .child(
                    self.render_home_section(
                        "newest",
                        "Newest albums",
                        self.render_home_album_content(
                            "newest",
                            &self.state.albums[..self.state.albums.len().min(30)],
                            self.state.albums_loading,
                            "No albums in your library yet.",
                            cx,
                            window,
                        ),
                        cx,
                    ),
                )
                .into_any_element(),
            View::Favorites => {
                let favorites = &self.state.favorites;
                let total =
                    favorites.artists.len() + favorites.albums.len() + favorites.songs.len();
                let mut content = v_flex()
                    .gap_5()
                    .child(self.page_header(
                        "Favorites",
                        format!("{total} starred items from your Navidrome library"),
                        cx,
                    ))
                    .children(self.error_banner(cx));

                if total == 0 {
                    content = content.child(
                        div()
                            .py_8()
                            .text_color(cx.theme().muted_foreground)
                            .child("Click a heart on a song, album, or artist to add it here."),
                    );
                }
                if !favorites.artists.is_empty() {
                    content = content
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Artists"),
                        )
                        .child(self.render_artist_grid(&favorites.artists, cx, window));
                }
                if !favorites.albums.is_empty() {
                    content = content
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Albums"),
                        )
                        .child(self.render_album_grid("favorites", &favorites.albums, cx, window));
                }
                if !favorites.songs.is_empty() {
                    content = content
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Songs"),
                        )
                        .child(self.render_song_list("favorites", false, cx));
                }

                content.into_any_element()
            }
            View::Artists => v_flex()
                .gap_5()
                .child(self.page_header(
                    "Artists",
                    format!("{} artists in your library", self.state.artists.len()),
                    cx,
                ))
                .children(self.error_banner(cx))
                .child(if self.state.artists.is_empty() && self.state.loading {
                    self.render_artist_skeleton_grid(cx)
                } else {
                    let visible = self.state.artists_visible.min(self.state.artists.len());
                    self.render_artist_grid(&self.state.artists[..visible], cx, window)
                })
                .into_any_element(),
            View::Albums => v_flex()
                .gap_5()
                .child(self.page_header(
                    "Albums",
                    format!("{} albums in your library", self.state.albums.len()),
                    cx,
                ))
                .children(self.error_banner(cx))
                .child(
                    if self.state.albums.is_empty() && self.state.albums_loading {
                        self.render_album_skeleton_grid(cx)
                    } else {
                        self.render_album_grid("albums", &self.state.albums, cx, window)
                    },
                )
                .into_any_element(),
            View::Playlists => {
                let visible_playlists =
                    self.state.playlists_visible.min(self.state.playlists.len());
                let playlist_list = v_flex()
                    .w_full()
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_lg()
                    .overflow_hidden()
                    .children(
                        self.state.playlists[..visible_playlists]
                            .iter()
                            .enumerate()
                            .map(|(index, playlist)| {
                                let playlist_for_click = playlist.clone();
                                let track_count = playlist.song_count.unwrap_or_default();
                                let details = playlist
                                    .owner
                                    .as_deref()
                                    .filter(|owner| !owner.trim().is_empty())
                                    .map(|owner| format!("{track_count} tracks | {owner}"))
                                    .unwrap_or_else(|| format!("{track_count} tracks"));

                                h_flex()
                                    .id(SharedString::from(format!("playlist-row-{}", playlist.id)))
                                    .h(px(68.0))
                                    .px_3()
                                    .gap_3()
                                    .items_center()
                                    .cursor_pointer()
                                    .when(index > 0, |this| {
                                        this.border_t_1().border_color(cx.theme().border)
                                    })
                                    .hover(|style| style.bg(cx.theme().accent.opacity(0.08)))
                                    .child(self.render_cover(
                                        playlist.cover_art.as_deref(),
                                        48.0,
                                        cx,
                                    ))
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .truncate()
                                                    .child(playlist.name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .truncate()
                                                    .child(details),
                                            ),
                                    )
                                    .child(
                                        Icon::new(AppIcon::ChevronRight)
                                            .small()
                                            .text_color(cx.theme().muted_foreground),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.open_playlist(playlist_for_click.clone());
                                        cx.notify();
                                    }))
                            }),
                    );

                v_flex()
                    .gap_5()
                    .child(self.page_header(
                        "Playlists",
                        format!("{} playlists", self.state.playlists.len()),
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("new-playlist")
                                    .label("New playlist")
                                    .primary()
                                    .small()
                                    .disabled(self.playlist_ui.busy || self.api.is_none())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.name_playlist(None, vec![], window, cx)
                                    })),
                            )
                            .child(
                                Button::new("refresh-playlists")
                                    .icon(AppIcon::Refresh)
                                    .label("Refresh")
                                    .outline()
                                    .small()
                                    .disabled(
                                        self.playlist_ui.busy || self.playlist_ui.list_loading,
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.load_playlists();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .children(self.error_banner(cx))
                    .when(
                        self.state.playlists.is_empty() && self.playlist_ui.list_loading,
                        |this| this.child(self.render_list_skeleton(8, cx)),
                    )
                    .when(
                        self.state.playlists.is_empty() && !self.playlist_ui.list_loading,
                        |this| {
                            this.child(
                                div()
                                    .py_8()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("No playlists are available."),
                            )
                        },
                    )
                    .when(!self.state.playlists.is_empty(), |this| {
                        this.child(playlist_list)
                    })
                    .into_any_element()
            }
            View::Search => {
                let mut content = v_flex()
                    .gap_5()
                    .child(self.page_header(
                        "Search",
                        "Find music across the entire server library.",
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .on_key_down(|_, _, cx| cx.stop_propagation())
                                    .child(Input::new(&self.search_input).w_full()),
                            )
                            .child(
                                Button::new("run-search")
                                    .label("Search")
                                    .primary()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.submit_search(cx);
                                        cx.notify();
                                    })),
                            ),
                    )
                    .children(self.error_banner(cx));
                if let Some(results) = &self.state.search_results {
                    if !results.artists.is_empty() {
                        content = content
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Artists"),
                            )
                            .child(self.render_artist_grid(&results.artists, cx, window));
                    }
                    if !results.albums.is_empty() {
                        content = content
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Albums"),
                            )
                            .child(self.render_album_grid("search", &results.albums, cx, window));
                    }
                    if !results.songs.is_empty() {
                        content = content
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Songs"),
                            )
                            .child(self.render_song_list("search", false, cx));
                    }
                }
                content.into_any_element()
            }
            View::ArtistDetail => {
                let Some(artist) = &self.state.current_artist else {
                    return div().child("Artist not found").into_any_element();
                };
                v_flex()
                    .gap_5()
                    .child(self.page_header(
                        artist.name.clone(),
                        format!("{} albums", self.state.artist_albums.len()),
                        cx,
                    ))
                    .children(self.error_banner(cx))
                    .child(self.render_album_grid(
                        "artist-detail",
                        &self.state.artist_albums,
                        cx,
                        window,
                    ))
                    .into_any_element()
            }
            View::AlbumDetail => {
                let Some(album) = &self.state.current_album else {
                    return div().child("Album not found").into_any_element();
                };
                v_flex()
                    .h_full()
                    .min_h_0()
                    .gap_5()
                    .child(
                        h_flex()
                            .gap_5()
                            .items_end()
                            .flex_none()
                            .child(self.render_cover(album.cover_art.as_deref(), 180.0, cx))
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_2xl()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(album.name.clone()),
                                    )
                                    .child(div().text_color(cx.theme().muted_foreground).child(
                                        format!(
                                                "{}{}",
                                                album.artist,
                                                album
                                                    .year
                                                    .map(|year| format!(" - {year}"))
                                                    .unwrap_or_default()
                                            ),
                                    ))
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .flex_wrap()
                                            .child(
                                                Button::new("play-album")
                                                    .label("Play album")
                                                    .primary()
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        let songs =
                                                            this.state.current_songs.clone();
                                                        if !songs.is_empty() {
                                                            this.play_song_list(&songs, 0);
                                                        }
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                Button::new("append-album")
                                                    .icon(AppIcon::Queue)
                                                    .label("Append all")
                                                    .outline()
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        let songs =
                                                            this.state.current_songs.clone();
                                                        this.append_all(&songs);
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                Button::new("album-add-to-playlist")
                                                    .label("Add to playlist…")
                                                    .outline()
                                                    .disabled(
                                                        self.playlist_ui.busy
                                                            || self.state.current_songs.is_empty(),
                                                    )
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.choose_playlist(
                                                                crate::playlist::song_ids(
                                                                    &this.state.current_songs,
                                                                ),
                                                                window,
                                                                cx,
                                                            );
                                                        },
                                                    )),
                                            ),
                                    ),
                            ),
                    )
                    .children(self.error_banner(cx))
                    .child(self.render_song_list("album-detail", false, cx))
                    .into_any_element()
            }
            View::PlaylistDetail => {
                let Some(playlist) = &self.state.current_playlist else {
                    return div().child("Playlist not found").into_any_element();
                };
                let rename = playlist.clone();
                let delete = playlist.clone();
                let read_only = !crate::playlist::can_edit(playlist, &self.config.username);
                let edit_disabled =
                    read_only || self.playlist_ui.busy || self.playlist_ui.detail_loading;
                v_flex()
                    .h_full()
                    .min_h_0()
                    .gap_5()
                    .child(self.page_header(
                        playlist.name.clone(),
                        format!("{} tracks", self.state.playlist_songs.len()),
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                Button::new("play-playlist")
                                    .icon(PlayerIcon::Play)
                                    .label("Play all")
                                    .small()
                                    .info()
                                    .outline()
                                    .disabled(
                                        self.state.playlist_songs.is_empty()
                                            || self.playlist_ui.detail_loading,
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let songs = this.state.playlist_songs.clone();
                                        if !songs.is_empty() {
                                            this.play_song_list(&songs, 0);
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("append-playlist")
                                    .icon(AppIcon::Queue)
                                    .label("Append all")
                                    .small()
                                    .outline()
                                    .disabled(
                                        self.state.playlist_songs.is_empty()
                                            || self.playlist_ui.detail_loading,
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let songs = this.state.playlist_songs.clone();
                                        this.append_all(&songs);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("rename-playlist")
                                    .label("Rename")
                                    .small()
                                    .outline()
                                    .disabled(edit_disabled)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.name_playlist(Some(rename.clone()), vec![], window, cx)
                                    })),
                            )
                            .child(
                                Button::new("delete-playlist")
                                    .label("Delete")
                                    .small()
                                    .ghost()
                                    .disabled(edit_disabled)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.delete_playlist(delete.clone(), window, cx)
                                    })),
                            )
                            .child(
                                Button::new("refresh-playlist")
                                    .icon(AppIcon::Refresh)
                                    .tooltip("Refresh playlist")
                                    .small()
                                    .ghost()
                                    .disabled(
                                        self.playlist_ui.busy || self.playlist_ui.detail_loading,
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(p) = &this.state.current_playlist {
                                            this.load_playlist_detail(p.id.clone());
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(if self.playlist_ui.busy {
                                "Saving playlist…"
                            } else if self.playlist_ui.detail_loading {
                                "Loading playlist…"
                            } else if read_only {
                                "Shared playlist · Only the owner can edit it."
                            } else if self.state.playlist_songs.is_empty() {
                                "This playlist is empty. Use a song’s actions menu to add tracks."
                            } else {
                                "Use a song’s actions menu to move or remove it."
                            }),
                    )
                    .children(self.error_banner(cx))
                    .child(self.render_song_list("playlist-detail", false, cx))
                    .into_any_element()
            }
            View::NowPlaying => self.render_now_playing(window, cx),
            View::Queue => v_flex()
                .h_full()
                .min_h_0()
                .gap_5()
                .child(self.page_header(
                    "Queue",
                    format!("{} tracks in the current queue", self.state.queue.len()),
                    cx,
                ))
                .when(self.state.queue.is_empty(), |this| {
                    this.child(
                        div()
                            .py_8()
                            .text_color(cx.theme().muted_foreground)
                            .child("Start playing an album or playlist to build a queue."),
                    )
                })
                .when(!self.state.queue.is_empty(), |this| {
                    this.child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                Button::new("queue-play-all")
                                    .icon(PlayerIcon::Play)
                                    .label("Play all")
                                    .small()
                                    .info()
                                    .outline()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let songs = this.state.queue.clone();
                                        if !songs.is_empty() {
                                            this.play_song_list(&songs, 0);
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("queue-clear-after")
                                    .label("Clear after current")
                                    .small()
                                    .outline()
                                    .disabled(self.state.queue_index.is_none())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.clear_queue_after_current();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("save-queue-playlist")
                                    .label("Save as playlist")
                                    .small()
                                    .outline()
                                    .disabled(self.playlist_ui.busy || self.api.is_none())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.name_playlist(
                                            None,
                                            crate::playlist::song_ids(&this.state.queue),
                                            window,
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                Button::new("queue-clear-all")
                                    .icon(AppIcon::Close)
                                    .label("Clear queue")
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.clear_queue();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(self.render_song_list("queue", true, cx))
                })
                .into_any_element(),
        }
    }

    fn render_mini(&mut self, window: &mut Window, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme().clone();
        let song = self.state.now_playing.clone();
        let has_track = song.is_some();
        let playback = self.audio.state();
        let is_playing = playback.active && !playback.paused;
        let duration = playback.duration.unwrap_or_default();
        let progress_percent = if duration.is_zero() {
            0.0
        } else {
            (playback.position.as_secs_f32() / duration.as_secs_f32() * 100.0).clamp(0.0, 100.0)
        };

        let cover_id = song.as_ref().and_then(|song| song.cover_art.as_deref());
        let cover = cover_id
            .and_then(|id| self.state.covers.get(id))
            .cloned()
            .unwrap_or_else(|| self.default_cover.clone());

        let (base, raw_accent) = self.now_playing_palette(cx);
        let accent = self.now_playing_accent(cx);
        let accent_fg = accent_foreground(accent);
        let play_button_style = ButtonCustomVariant::new(cx)
            .color(accent)
            .foreground(accent_fg)
            .hover(adjust_lightness(accent, 0.05))
            .active(adjust_lightness(accent, -0.05))
            .shadow(true);

        let light = theme.background.l >= 0.5;
        let base_strength = if light { 0.36 } else { 0.3 };
        let accent_strength = if light { 0.28 } else { 0.24 };
        let bg_start = theme.background.blend(base.opacity(base_strength));
        let bg_end = theme.background.blend(raw_accent.opacity(accent_strength));
        let bg_animation = Animation::new(Duration::from_secs(20)).repeat();

        let title = song
            .as_ref()
            .map(|song| song.title.clone())
            .unwrap_or_else(|| "No track playing".to_string());
        let artist = song
            .as_ref()
            .map(|song| song.artist.clone())
            .unwrap_or_else(|| "Choose a song from your library.".to_string());

        let cover_element = img(cover)
            // `on_click` only fires for elements with an id: it needs element state
            // to track the press/release pair.
            .id("mini-cover")
            .size(px(26.0))
            .aspect_square()
            .object_fit(ObjectFit::Cover)
            .flex_none()
            .rounded_md()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| {
                this.exit_mini_mode(window, cx);
                this.open_now_playing();
            }))
            .occlude();

        // Fixed widths in the mini row that leave room for the scrolling info text:
        // px_1p5 padding (12) + cover (26) + gap_1p5 between the 4 row children (18)
        // + controls (16+2+22+2+16 = 58) + expand button (16) = 130. On Linux the
        // window shadow shrinks the content, so use the content (inner) width.
        let paddings = window_paddings(window);
        let content_width = window.viewport_size().width - paddings.left - paddings.right;
        let info_width = f32::from(content_width - px(130.0)).max(1.0);
        let track_id = song
            .as_ref()
            .map(|song| song.id.clone())
            .unwrap_or_default();
        let title_id = SharedString::from(format!("mini-title-{track_id}"));
        let artist_id = SharedString::from(format!("mini-artist-{track_id}"));

        let info = v_flex()
            .flex_1()
            .min_w_0()
            .gap_0p5()
            .overflow_hidden()
            .id("mini-now-playing-info")
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| {
                this.exit_mini_mode(window, cx);
                this.open_now_playing();
            }))
            .occlude()
            .child(self.render_scrolling_text(
                title.clone().into(),
                info_width,
                rems(0.75).to_pixels(window.rem_size()),
                FontWeight::SEMIBOLD,
                title_id,
                window,
            ))
            .child(
                div()
                    .text_color(theme.muted_foreground)
                    .child(self.render_scrolling_text(
                        artist.clone().into(),
                        info_width,
                        px(10.0),
                        FontWeight::NORMAL,
                        artist_id,
                        window,
                    )),
            );

        let controls = h_flex()
            .flex_none()
            .items_center()
            .gap_0p5()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .occlude()
            .child(
                Button::new("mini-previous")
                    .icon(PlayerIcon::Previous)
                    .tooltip("Previous track")
                    .ghost()
                    .with_size(px(16.0))
                    .rounded_full()
                    .disabled(!has_track)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.skip(-1);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("mini-play-pause")
                    .icon(if is_playing {
                        PlayerIcon::Pause
                    } else {
                        PlayerIcon::Play
                    })
                    .tooltip(if is_playing { "Pause" } else { "Play" })
                    .custom(play_button_style)
                    .with_size(px(22.0))
                    .rounded_full()
                    .disabled(!has_track)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_playback();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("mini-next")
                    .icon(PlayerIcon::Next)
                    .tooltip("Next track")
                    .ghost()
                    .with_size(px(16.0))
                    .rounded_full()
                    .disabled(!has_track)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.skip(1);
                        cx.notify();
                    })),
            );

        let progress_line = div()
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .h(px(2.0))
            .bg(accent.opacity(0.12))
            .child(
                div()
                    .h_full()
                    .w(relative(progress_percent / 100.0))
                    .bg(accent.opacity(0.45)),
            );

        let expand_button = Button::new("mini-expand")
            .icon(AppIcon::Maximize)
            .tooltip("Exit mini player")
            .ghost()
            .with_size(px(16.0))
            .rounded_full()
            .on_click(cx.listener(|this, _, window, cx| {
                this.exit_mini_mode(window, cx);
            }));

        // The mini player root is a window drag area, so the expand button needs to
        // block the drag behind it (like the `controls` row) or its clicks would be
        // turned into window moves.
        let expand_button = h_flex()
            .flex_none()
            .items_center()
            .occlude()
            .child(expand_button);

        v_flex()
            .relative()
            .size_full()
            .border_1()
            .border_color(theme.border.opacity(0.5))
            .text_color(theme.foreground)
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                let delta_y = f32::from(event.delta.pixel_delta(px(16.0)).y);
                if delta_y.abs() < f32::EPSILON {
                    return;
                }
                let delta = if delta_y > 0.0 { 0.05 } else { -0.05 };
                this.adjust_volume(delta, window, cx);
                cx.stop_propagation();
            }))
            .when(cfg!(target_os = "windows"), |this| {
                this.window_control_area(WindowControlArea::Drag)
            })
            .when(cfg!(not(target_os = "windows")), |this| {
                this.on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
            })
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .px_1p5()
                    .gap_1p5()
                    .items_center()
                    .child(cover_element)
                    .child(info)
                    .child(controls)
                    .child(expand_button),
            )
            .child(progress_line)
            .with_animation(
                "mini-player-background",
                bg_animation,
                move |this, delta| {
                    this.bg(linear_gradient(
                        120.0 + delta * 360.0,
                        linear_color_stop(bg_start, 0.0),
                        linear_color_stop(bg_end, 1.0),
                    ))
                },
            )
            .into_any_element()
    }

    fn render_player(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let playback = self.audio.state();
        let accent = self.now_playing_accent(cx);
        let accent_foreground = accent_foreground(accent);
        let play_button_style = ButtonCustomVariant::new(cx)
            .color(accent)
            .foreground(accent_foreground)
            .hover(adjust_lightness(accent, 0.05))
            .active(adjust_lightness(accent, -0.05))
            .shadow(true);
        let duration = playback.duration.unwrap_or_default();
        let buffered_percent = if duration.is_zero() {
            0.0
        } else {
            (playback.buffered.as_secs_f32() / duration.as_secs_f32() * 100.0).clamp(0.0, 100.0)
        };
        let lyrics_open = self.state.view == View::NowPlaying;
        let queue_open = self.state.view == View::Queue;
        let output_volume = effective_volume(self.config.volume, self.muted);
        let volume_icon = if self.muted || output_volume <= 0.001 {
            AppIcon::VolumeMuted
        } else if output_volume < 0.5 {
            AppIcon::VolumeLow
        } else {
            AppIcon::VolumeHigh
        };
        let volume_value = match self.volume_slider.read(cx).value() {
            SliderValue::Single(value) => value,
            SliderValue::Range(_, value) => value,
        };
        let technical_info = self
            .state
            .now_playing
            .as_ref()
            .zip(self.state.now_playing_quality)
            .map(|(song, quality)| playback_technical_info(song, quality));
        let song_info = if let Some(song) = &self.state.now_playing {
            h_flex()
                .id("now-playing-info")
                .w(px(260.0))
                .flex_shrink_0()
                .min_w_0()
                .overflow_hidden()
                .gap_3()
                .p_1()
                .items_center()
                .cursor_pointer()
                .rounded(px(12.0))
                .hover(|style| style.bg(cx.theme().accent.opacity(0.08)))
                .child(self.render_cover(song.cover_art.as_deref(), 60.0, cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(song.title.clone()),
                        )
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .overflow_hidden()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .truncate()
                                        .child(song.artist.clone()),
                                )
                                .child(
                                    Button::new("player-lyrics-shortcut")
                                        .icon(AppIcon::Lyrics)
                                        .tooltip("Lyrics")
                                        .ghost()
                                        .with_size(px(26.0))
                                        .rounded_full()
                                        .selected(lyrics_open)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.open_now_playing();
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.open_now_playing();
                    cx.notify();
                }))
                .into_any_element()
        } else {
            h_flex()
                .w(px(260.0))
                .flex_shrink_0()
                .min_w_0()
                .overflow_hidden()
                .gap_3()
                .p_1()
                .items_center()
                .child(self.render_cover(None, 60.0, cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child("No track playing"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child("Choose a song from your library."),
                        ),
                )
                .into_any_element()
        };

        let right_controls = h_flex()
            .w(px(260.0))
            .h(px(40.0))
            .min_w_0()
            .flex_shrink_0()
            .justify_end()
            .items_center()
            .gap_2()
            .pl_3()
            .border_l_1()
            .border_color(cx.theme().border.opacity(0.8))
            .when_some(technical_info, |this, info| {
                let tooltip = info.tooltip.clone();
                this.child(
                    h_flex()
                        .id("player-technical-info")
                        .min_w_0()
                        .gap_0p5()
                        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                        .child(technical_info_chip(
                            "player-format",
                            info.format,
                            accent,
                            cx,
                        )),
                )
                .child(
                    div()
                        .h(px(24.0))
                        .border_l_1()
                        .border_color(cx.theme().border.opacity(0.6)),
                )
            })
            .when_some(self.state.now_playing.as_ref(), |this, song| {
                this.child(
                    self.favorite_button(FavoriteKind::Song, &song.id, cx)
                        .with_size(px(30.0)),
                )
            })
            .child(self.playback_mode_button(cx))
            .child(
                Button::new("player-queue")
                    .icon(AppIcon::Queue)
                    .tooltip("Queue")
                    .ghost()
                    .with_size(px(30.0))
                    .rounded_full()
                    .selected(queue_open)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.settings_open = false;
                        this.state.view = View::Queue;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .h(px(24.0))
                    .border_l_1()
                    .border_color(cx.theme().border.opacity(0.6)),
            )
            .child(
                div()
                    .id("player-volume-control")
                    .relative()
                    .flex_none()
                    .on_hover(cx.listener(|this, hovering, _, cx| {
                        if *hovering {
                            this.show_volume_panel(cx);
                        } else {
                            this.schedule_volume_panel_close(cx);
                        }
                    }))
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                        let delta_y = f32::from(event.delta.pixel_delta(px(16.0)).y);
                        if delta_y.abs() < f32::EPSILON {
                            return;
                        }
                        let delta = if delta_y > 0.0 { 0.05 } else { -0.05 };
                        this.adjust_volume(delta, window, cx);
                        cx.stop_propagation();
                    }))
                    .when(self.volume_panel_open, |this| {
                        this.child(
                            v_flex()
                                .id("player-volume-panel")
                                .absolute()
                                .bottom(px(64.0))
                                .left(px(-13.0))
                                .w(px(54.0))
                                .p_2()
                                .items_center()
                                .gap_1()
                                .rounded(px(6.0))
                                .border_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().popover)
                                .text_color(cx.theme().popover_foreground)
                                .shadow_md()
                                .capture_any_mouse_down(cx.listener(|this, _, _, cx| {
                                    this.begin_volume_panel_drag(cx);
                                }))
                                .capture_any_mouse_up(cx.listener(|this, _, _, cx| {
                                    this.end_volume_panel_drag(true, cx);
                                }))
                                .on_mouse_up_out(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| {
                                        this.end_volume_panel_drag(false, cx);
                                    }),
                                )
                                .on_hover(cx.listener(|this, hovering, _, cx| {
                                    if *hovering {
                                        this.show_volume_panel(cx);
                                    } else {
                                        this.schedule_volume_panel_close(cx);
                                    }
                                }))
                                .child(
                                    div()
                                        .h(px(18.0))
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(if self.muted {
                                            cx.theme().muted_foreground
                                        } else {
                                            cx.theme().popover_foreground
                                        })
                                        .child(format!("{}%", volume_value.round() as u32)),
                                )
                                .child(
                                    div()
                                        .h(px(120.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            Slider::new(&self.volume_slider)
                                                .vertical()
                                                .h(px(120.0))
                                                .bg(accent)
                                                .text_color(accent)
                                                .rounded_full(),
                                        ),
                                ),
                        )
                    })
                    .child(
                        Button::new("player-volume-mute")
                            .icon(volume_icon)
                            .ghost()
                            .with_size(px(28.0))
                            .rounded_full()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_mute(window, cx);
                            })),
                    ),
            );

        h_flex()
            .h(px(88.0))
            .px_4()
            .py_2()
            .gap_4()
            .items_center()
            .justify_between()
            .border_t_1()
            .border_color(cx.theme().border.opacity(0.8))
            .bg(cx.theme().secondary.opacity(0.22))
            .child(song_info)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .gap_1()
                    .items_center()
                    .child(
                        h_flex()
                            .gap_3()
                            .items_center()
                            .child(
                                Button::new("previous")
                                    .icon(PlayerIcon::Previous)
                                    .tooltip("Previous track")
                                    .ghost()
                                    .with_size(px(28.0))
                                    .rounded_full()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.skip(-1);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("play-pause")
                                    .icon(if playback.active && !playback.paused {
                                        PlayerIcon::Pause
                                    } else {
                                        PlayerIcon::Play
                                    })
                                    .tooltip(if playback.active && !playback.paused {
                                        "Pause"
                                    } else {
                                        "Play"
                                    })
                                    .custom(play_button_style)
                                    .with_size(px(40.0))
                                    .rounded_full()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_playback();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("next")
                                    .icon(PlayerIcon::Next)
                                    .tooltip("Next track")
                                    .ghost()
                                    .with_size(px(28.0))
                                    .rounded_full()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.skip(1);
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .h(px(24.0))
                            .max_w(px(760.0))
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                div()
                                    .w(px(40.0))
                                    .flex_none()
                                    .text_right()
                                    .child(format_playback(playback.position)),
                            )
                            .child(
                                div()
                                    .relative()
                                    .flex_1()
                                    .mx_1()
                                    .h_6()
                                    .child(
                                        div()
                                            .absolute()
                                            .left_0()
                                            .top(px(9.0))
                                            .h_1p5()
                                            .w(relative(buffered_percent / 100.0))
                                            .rounded_full()
                                            .bg(accent.opacity(0.24)),
                                    )
                                    .child(
                                        Slider::new(&self.playback_slider)
                                            .w_full()
                                            .bg(accent)
                                            .text_color(accent)
                                            .rounded_full(),
                                    ),
                            )
                            .child(
                                div()
                                    .w(px(40.0))
                                    .flex_none()
                                    .child(format_playback(duration)),
                            ),
                    ),
            )
            .child(right_controls)
            .into_any_element()
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

        if self.state.settings_open {
            return v_flex()
                .size_full()
                .bg(cx.theme().background)
                .text_color(cx.theme().foreground)
                .child(
                    TitleBar::new().child(
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
                                    .child("Settings"),
                            )
                            .child(
                                // See `toggle-mini-player`: the title bar drag area
                                // must not cover clickable elements.
                                div().occlude().child(
                                    Button::new("close-settings")
                                        .icon(AppIcon::Close)
                                        .tooltip("Close settings")
                                        .ghost()
                                        .small()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.state.settings_open = false;
                                            cx.notify();
                                        })),
                                ),
                            ),
                    ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .p_6()
                        .child(self.render_settings(cx)),
                )
                .into_any_element();
        }

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
                                                cx.notify();
                                            })),
                                    ),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .child(self.render_now_playing(window, cx)),
                )
                .child(self.render_player(cx))
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
                TitleBar::new().child(
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
                                            this.state.settings_open = !this.state.settings_open;
                                            cx.notify();
                                        })),
                                ),
                        ),
                ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        v_flex()
                            .w(px(216.0))
                            .h_full()
                            .p_3()
                            .gap_1()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().sidebar)
                            .child(
                                div()
                                    .px_2()
                                    .py_2()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(cx.theme().muted_foreground)
                                    .child("LIBRARY"),
                            )
                            .child(self.nav_button(View::Home, "Home", cx))
                            .child(self.nav_button(View::Favorites, "Favorites", cx))
                            .child(self.nav_button(View::Artists, "Artists", cx))
                            .child(self.nav_button(View::Albums, "Albums", cx))
                            .child(self.nav_button(View::Playlists, "Playlists", cx))
                            .child(self.nav_button(View::Search, "Search", cx)),
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
                    ),
            )
            .child(self.render_player(cx))
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

#[cfg(not(target_os = "windows"))]
fn request_window_close(_hwnd: isize) {}

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

fn normalize_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        0.7
    }
}

fn effective_volume(volume: f32, muted: bool) -> f32 {
    if muted {
        0.0
    } else {
        normalize_volume(volume)
    }
}

fn restored_volume(volume_before_mute: f32) -> f32 {
    let volume = normalize_volume(volume_before_mute);
    if volume > 0.001 {
        volume
    } else {
        0.7
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
        .text_size(px(11.0))
        .text_color(cx.theme().foreground.opacity(0.74))
        .truncate()
        .child(label)
        .into_any_element()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PlaybackTechnicalInfo {
    format: String,
    tooltip: String,
}

fn playback_technical_info(song: &Song, quality: TranscodingQuality) -> PlaybackTechnicalInfo {
    let source = source_technical_info(song);
    if let Some(bit_rate) = quality.max_bit_rate() {
        let estimated_bytes = song.duration.and_then(|seconds| {
            let seconds = u64::try_from(seconds).ok()?;
            Some(
                seconds
                    .saturating_mul(u64::from(bit_rate))
                    .saturating_mul(1_000)
                    / 8,
            )
        });
        let size = estimated_bytes
            .map(|bytes| format!("~{}", format_file_size(bytes)))
            .unwrap_or_else(|| "Unknown size".to_string());
        let bit_rate = format!("{bit_rate} kbps");
        let display = format!("MP3 · {bit_rate} · {size}");
        return PlaybackTechnicalInfo {
            format: "MP3".to_string(),
            tooltip: format!("Current stream: {display}\nSource file: {}", source.display),
        };
    }

    PlaybackTechnicalInfo {
        format: source.format,
        tooltip: format!("Current stream: {}", source.display),
    }
}

struct SourceTechnicalInfo {
    format: String,
    display: String,
}

fn source_technical_info(song: &Song) -> SourceTechnicalInfo {
    let format = song
        .suffix
        .as_deref()
        .filter(|suffix| !suffix.trim().is_empty())
        .map(|suffix| suffix.trim().to_ascii_uppercase())
        .or_else(|| {
            song.content_type
                .as_deref()
                .and_then(format_from_content_type)
        })
        .unwrap_or_else(|| "AUDIO".to_string());
    let bit_rate = song
        .bit_rate
        .and_then(|bit_rate| u32::try_from(bit_rate).ok())
        .filter(|bit_rate| *bit_rate > 0)
        .map(|bit_rate| format!("{bit_rate} kbps"))
        .or_else(|| estimated_bit_rate(song).map(|bit_rate| format!("~{bit_rate} kbps")))
        .unwrap_or_else(|| "Unknown rate".to_string());
    let size = song
        .size
        .and_then(|size| u64::try_from(size).ok())
        .map(format_file_size)
        .unwrap_or_else(|| "Unknown size".to_string());
    let display = format!("{format} · {bit_rate} · {size}");

    SourceTechnicalInfo { format, display }
}

fn format_from_content_type(content_type: &str) -> Option<String> {
    let subtype = content_type
        .split(';')
        .next()?
        .trim()
        .rsplit('/')
        .next()?
        .trim_start_matches("x-");
    if subtype.is_empty() {
        return None;
    }
    Some(match subtype.to_ascii_lowercase().as_str() {
        "mpeg" | "mp3" => "MP3".to_string(),
        "mp4" | "aac" => "AAC".to_string(),
        "ogg" | "vorbis" => "OGG".to_string(),
        other => other.to_ascii_uppercase(),
    })
}

fn estimated_bit_rate(song: &Song) -> Option<u64> {
    let bytes = u64::try_from(song.size?).ok()?;
    let seconds = u64::try_from(song.duration?).ok()?;
    (seconds > 0).then(|| bytes.saturating_mul(8) / seconds / 1_000)
}

fn format_file_size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GIB {
        format!("{:.2} GB", bytes / GIB)
    } else if bytes >= MIB {
        format!("{:.1} MB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KB", bytes / KIB)
    } else {
        format!("{} B", bytes as u64)
    }
}

fn relative_luminance(color: Hsla) -> f32 {
    fn linearize(channel: f32) -> f32 {
        if channel <= 0.04045 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    }

    let rgb = color.to_rgb();
    0.2126 * linearize(rgb.r) + 0.7152 * linearize(rgb.g) + 0.0722 * linearize(rgb.b)
}

fn contrast_ratio(a: Hsla, b: Hsla) -> f32 {
    let brighter = relative_luminance(a).max(relative_luminance(b));
    let darker = relative_luminance(a).min(relative_luminance(b));
    (brighter + 0.05) / (darker + 0.05)
}

pub(crate) fn readable_accent(mut accent: Hsla, background: Hsla) -> Hsla {
    accent.s = accent.s.clamp(0.38, 0.78);
    accent.a = 1.0;
    let lighten = relative_luminance(background) < 0.35;
    accent.l = if lighten {
        accent.l.clamp(0.56, 0.74)
    } else {
        accent.l.clamp(0.26, 0.46)
    };

    for _ in 0..12 {
        if contrast_ratio(accent, background) >= 4.5 {
            break;
        }
        accent.l = if lighten {
            (accent.l + 0.035).min(0.88)
        } else {
            (accent.l - 0.035).max(0.12)
        };
    }
    accent
}

pub(crate) fn accent_foreground(accent: Hsla) -> Hsla {
    let black = hsla(0.0, 0.0, 0.06, 1.0);
    let white = hsla(0.0, 0.0, 0.98, 1.0);
    if contrast_ratio(black, accent) >= contrast_ratio(white, accent) {
        black
    } else {
        white
    }
}

pub(crate) fn adjust_lightness(mut color: Hsla, amount: f32) -> Hsla {
    color.l = (color.l + amount).clamp(0.0, 1.0);
    color
}

fn rgb_to_hsla(red: f32, green: f32, blue: f32) -> Hsla {
    let max = red.max(green).max(blue);
    let min = red.min(green).min(blue);
    let lightness = (max + min) * 0.5;
    let delta = max - min;

    if delta <= f32::EPSILON {
        return hsla(0.0, 0.0, lightness, 1.0);
    }

    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs()).max(f32::EPSILON);
    let hue = if max == red {
        ((green - blue) / delta).rem_euclid(6.0)
    } else if max == green {
        (blue - red) / delta + 2.0
    } else {
        (red - green) / delta + 4.0
    } / 6.0;

    hsla(hue, saturation, lightness, 1.0)
}

fn extract_cover_palette(bytes: &[u8]) -> Option<(Hsla, Hsla)> {
    let image = image::load_from_memory(bytes)
        .ok()?
        .thumbnail(32, 32)
        .to_rgb8();
    let mut average = [0.0_f32; 3];
    let mut vivid = [0.0_f32; 3];
    let mut count = 0.0_f32;
    let mut vivid_weight = 0.0_f32;

    for pixel in image.pixels() {
        let red = f32::from(pixel[0]) / 255.0;
        let green = f32::from(pixel[1]) / 255.0;
        let blue = f32::from(pixel[2]) / 255.0;
        average[0] += red;
        average[1] += green;
        average[2] += blue;
        count += 1.0;

        let color = rgb_to_hsla(red, green, blue);
        if color.s > 0.25 && color.l > 0.12 && color.l < 0.88 {
            let weight = color.s * (1.0 - (color.l - 0.5).abs());
            vivid[0] += red * weight;
            vivid[1] += green * weight;
            vivid[2] += blue * weight;
            vivid_weight += weight;
        }
    }

    if count == 0.0 {
        return None;
    }

    let mut base = rgb_to_hsla(average[0] / count, average[1] / count, average[2] / count);
    let mut accent = if vivid_weight > 0.0 {
        rgb_to_hsla(
            vivid[0] / vivid_weight,
            vivid[1] / vivid_weight,
            vivid[2] / vivid_weight,
        )
    } else {
        base
    };

    base.s = base.s.clamp(0.2, 0.72);
    base.l = base.l.clamp(0.35, 0.62);
    accent.s = (accent.s * 1.12).clamp(0.32, 0.86);
    accent.l = accent.l.clamp(0.38, 0.6);
    if (base.h - accent.h).abs() < 0.035 {
        accent.h = (accent.h + 0.08) % 1.0;
    }

    Some((base, accent))
}

fn gpui_image_format(format: image::ImageFormat) -> Option<GpuiImageFormat> {
    match format {
        image::ImageFormat::Png => Some(GpuiImageFormat::Png),
        image::ImageFormat::Jpeg => Some(GpuiImageFormat::Jpeg),
        image::ImageFormat::WebP => Some(GpuiImageFormat::Webp),
        image::ImageFormat::Gif => Some(GpuiImageFormat::Gif),
        image::ImageFormat::Bmp => Some(GpuiImageFormat::Bmp),
        image::ImageFormat::Tiff => Some(GpuiImageFormat::Tiff),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use gpui_kit::hsla;

    use super::{
        accent_foreground, advance_index, album_page_offset, contrast_ratio, effective_volume,
        extract_cover_palette, format_file_size, loudness_gain_factor, match_shortcut,
        normalize_volume, playback_technical_info, queue_index_after_move,
        queue_index_after_remove, readable_accent, replay_gain_factor, restored_volume,
        seek_target, should_scrobble, Shortcut, ShuffleHistory,
    };
    use crate::models::{PlaybackMode, Song, TranscodingQuality, VolumeNormalization};
    use gpui_kit::Modifiers;

    #[test]
    fn preloading_waits_for_safe_current_audio_buffer_and_near_end() {
        let mut playback = crate::audio::PlaybackState {
            active: true,
            duration: Some(Duration::from_secs(180)),
            position: Duration::from_secs(165),
            fully_buffered: true,
            ..Default::default()
        };
        assert!(super::should_preload_next(&playback));
        playback.fully_buffered = false;
        assert!(!super::should_preload_next(&playback));
        playback.fully_buffered = true;
        playback.position = Duration::from_secs(164);
        assert!(!super::should_preload_next(&playback));
        playback.position = Duration::from_secs(165);
        playback.paused = true;
        assert!(!super::should_preload_next(&playback));
        playback.paused = false;
        playback.duration = None;
        assert!(!super::should_preload_next(&playback));
        playback.duration = Some(playback.position);
        assert!(!super::should_preload_next(&playback));
        playback.duration = Some(Duration::from_secs(180));
        playback.error = Some("stream failed".into());
        assert!(!super::should_preload_next(&playback));
    }

    #[test]
    fn preload_target_matches_playback_modes_and_manual_repeat_one_skip() {
        for (mode, current, natural, expected) in [
            (PlaybackMode::Sequential, 0, true, Some(1)),
            (PlaybackMode::Sequential, 2, true, None),
            (PlaybackMode::RepeatAll, 2, true, Some(0)),
            (PlaybackMode::RepeatOne, 1, true, Some(1)),
            (PlaybackMode::RepeatOne, 1, false, Some(2)),
            (PlaybackMode::RepeatOne, 2, false, Some(0)),
        ] {
            assert_eq!(
                super::next_queue_index(mode, current, 3, natural, None, None, || panic!(
                    "non-shuffle selected randomly"
                )),
                expected
            );
        }
        assert_eq!(
            super::next_queue_index(PlaybackMode::RepeatAll, 0, 0, true, None, None, || 0),
            None
        );
        assert_eq!(
            super::next_queue_index(PlaybackMode::RepeatOne, 0, 1, true, None, None, || 0),
            Some(0)
        );
    }

    #[test]
    fn shuffle_reservation_and_forward_history_do_not_draw_a_second_random_song() {
        let mut history = ShuffleHistory::default();
        history.start(0);
        assert_eq!(history.previous(), None);
        assert_eq!(history.snapshot(), (vec![0], vec![]));
        history.advance(2);
        assert_eq!(history.previous(), Some(0));
        let before = history.snapshot();
        let next = super::next_queue_index(
            PlaybackMode::Shuffle,
            0,
            4,
            true,
            history.forward.last().copied(),
            Some(3),
            || panic!("forward history has priority"),
        );
        assert_eq!(next, Some(2));
        assert_eq!(history.snapshot(), before);
        assert_eq!(history.restore_forward(), next);
        assert_eq!(
            super::next_queue_index(PlaybackMode::Shuffle, 2, 4, true, None, Some(3), || panic!(
                "reserved choice must be reused"
            )),
            Some(3)
        );
        assert_eq!(
            super::next_queue_index(
                PlaybackMode::Shuffle,
                2,
                4,
                false,
                None,
                Some(3),
                || panic!("manual next uses the same reservation")
            ),
            Some(3)
        );
    }

    #[test]
    fn queue_edits_remap_shuffle_history_by_occurrence() {
        // Positions 0 and 2 may have identical song IDs; their history is distinct.
        let mut history = ShuffleHistory::restore(vec![0, 2, 1], vec![3]);
        history.remap(|i| Some(if i >= 1 { i + 1 } else { i }));
        assert_eq!(history.snapshot(), (vec![0, 3, 2], vec![4]));
        history.remap(|i| Some(queue_index_after_move(3, 0, i)));
        assert_eq!(history.snapshot(), (vec![1, 0, 3], vec![4]));
        history.remap(|i| if i == 0 { None } else { Some(i - 1) });
        assert_eq!(history.snapshot(), (vec![0, 2], vec![3]));
    }

    #[test]
    fn duplicate_items_have_distinct_hover_and_element_keys() {
        let first = super::item_instance_key("recent", "album", 0, "same-id");
        assert_ne!(
            first,
            super::item_instance_key("frequent", "album", 0, "same-id")
        );
        assert_ne!(
            first,
            super::item_instance_key("recent", "album", 1, "same-id")
        );
        assert_ne!(
            first,
            super::item_instance_key("recent", "song", 0, "same-id")
        );
        assert_ne!(
            super::item_instance_key("queue", "song", 0, "repeat"),
            super::item_instance_key("queue", "song", 1, "repeat")
        );
    }

    #[test]
    fn cover_accent_remains_readable_on_light_and_dark_backgrounds() {
        let source = hsla(0.15, 0.9, 0.62, 1.0);
        let light = hsla(0.0, 0.0, 0.96, 1.0);
        let dark = hsla(0.0, 0.0, 0.08, 1.0);

        assert!(contrast_ratio(readable_accent(source, light), light) >= 4.5);
        assert!(contrast_ratio(readable_accent(source, dark), dark) >= 4.5);
    }

    #[test]
    fn accent_button_foreground_prefers_the_stronger_contrast() {
        let accent = hsla(0.12, 0.7, 0.72, 1.0);
        let foreground = accent_foreground(accent);
        assert!(contrast_ratio(foreground, accent) >= 4.5);
    }

    #[test]
    fn extracts_palette_from_default_cover() {
        let palette = extract_cover_palette(include_bytes!("../assets/default-cover.png"));
        assert!(palette.is_some());
    }

    #[test]
    fn displays_original_stream_metadata() {
        let song = Song {
            suffix: Some("flac".to_string()),
            bit_rate: Some(941),
            size: Some(25_165_824),
            duration: Some(214),
            ..Song::default()
        };

        let info = playback_technical_info(&song, TranscodingQuality::Original);

        assert_eq!(info.format, "FLAC");
        assert_eq!(info.tooltip, "Current stream: FLAC · 941 kbps · 24.0 MB");
    }

    #[test]
    fn displays_transcoded_stream_profile_and_estimated_size() {
        let song = Song {
            suffix: Some("flac".to_string()),
            size: Some(25_165_824),
            duration: Some(240),
            ..Song::default()
        };

        let info = playback_technical_info(&song, TranscodingQuality::Kbps192);

        assert_eq!(info.format, "MP3");
        assert_eq!(
            info.tooltip,
            "Current stream: MP3 · 192 kbps · ~5.5 MB\nSource file: FLAC · ~838 kbps · 24.0 MB"
        );
    }

    #[test]
    fn formats_file_sizes_for_compact_player_display() {
        assert_eq!(format_file_size(800), "800 B");
        assert_eq!(format_file_size(1_536), "1.5 KB");
        assert_eq!(format_file_size(3 * 1024 * 1024), "3.0 MB");
    }

    #[test]
    fn normalizes_volume_to_a_safe_output_range() {
        assert!((normalize_volume(0.42) - 0.42).abs() < f32::EPSILON);
        assert_eq!(normalize_volume(-0.5), 0.0);
        assert_eq!(normalize_volume(1.5), 1.0);
        assert!((normalize_volume(f32::NAN) - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn muting_only_changes_effective_output_volume() {
        assert!((effective_volume(0.64, false) - 0.64).abs() < f32::EPSILON);
        assert_eq!(effective_volume(0.64, true), 0.0);
    }

    #[test]
    fn restores_a_safe_nonzero_volume_after_muting() {
        assert!((restored_volume(0.42) - 0.42).abs() < f32::EPSILON);
        assert!((restored_volume(0.0) - 0.7).abs() < f32::EPSILON);
        assert!((restored_volume(f32::NAN) - 0.7).abs() < f32::EPSILON);
    }

    #[test]
    fn sequential_mode_stops_at_queue_boundaries() {
        assert_eq!(advance_index(2, 3, true, false), None); // 末尾 next → 不循环
        assert_eq!(advance_index(0, 3, false, false), None); // 开头 prev → 不循环
        assert_eq!(advance_index(1, 3, true, false), Some(2)); // 中间 next → 下一首
    }

    #[test]
    fn repeat_modes_wrap_around_queue() {
        assert_eq!(advance_index(2, 3, true, true), Some(0));
        assert_eq!(advance_index(0, 3, false, true), Some(2));
        assert_eq!(advance_index(1, 3, true, true), Some(2));
    }

    #[test]
    fn removing_item_before_current_shifts_index_down() {
        assert_eq!(queue_index_after_remove(0, Some(2), 3), Some(1));
        assert_eq!(queue_index_after_remove(1, Some(4), 4), Some(3));
    }

    #[test]
    fn removing_current_falls_back_to_same_slot() {
        // 删除正在播放的歌曲后，改播原位置的下一首。
        assert_eq!(queue_index_after_remove(2, Some(2), 2), Some(1));
        assert_eq!(queue_index_after_remove(1, Some(1), 3), Some(1));
    }

    #[test]
    fn removing_last_current_clears_the_queue_index() {
        assert_eq!(queue_index_after_remove(0, Some(0), 0), None);
    }

    #[test]
    fn removing_item_after_current_keeps_index() {
        assert_eq!(queue_index_after_remove(3, Some(1), 3), Some(1));
        assert_eq!(queue_index_after_remove(2, None, 3), None);
    }

    #[test]
    fn moving_item_before_current_to_after_shifts_index_down() {
        assert_eq!(queue_index_after_move(0, 3, 2), 1);
        assert_eq!(queue_index_after_move(1, 3, 2), 1);
    }

    #[test]
    fn moving_item_after_current_to_before_shifts_index_up() {
        assert_eq!(queue_index_after_move(3, 0, 2), 3);
    }

    #[test]
    fn moving_current_repositions_the_index() {
        assert_eq!(queue_index_after_move(2, 0, 2), 0);
        assert_eq!(queue_index_after_move(0, 3, 0), 3);
        assert_eq!(queue_index_after_move(2, 3, 2), 3);
    }

    #[test]
    fn adjacent_moves_keep_relative_position() {
        assert_eq!(queue_index_after_move(1, 2, 2), 1);
        assert_eq!(queue_index_after_move(1, 2, 1), 2);
    }

    #[test]
    fn scrobbles_at_half_duration_or_four_minutes() {
        assert!(!should_scrobble(
            Duration::from_secs(10),
            Some(Duration::from_secs(30))
        ));
        assert!(should_scrobble(
            Duration::from_secs(15),
            Some(Duration::from_secs(30))
        ));
        assert!(should_scrobble(
            Duration::from_secs(241),
            Some(Duration::from_secs(600))
        ));
        assert!(should_scrobble(Duration::from_secs(240), None));
        assert!(!should_scrobble(Duration::from_secs(120), None));
        assert!(!should_scrobble(
            Duration::ZERO,
            Some(Duration::from_secs(300))
        ));
    }

    #[test]
    fn replay_gain_applies_db_gain_in_track_mode() {
        // -3.5 dB ≈ 0.668；0 dB = 1.0；+6 dB ≈ 1.995
        let factor = replay_gain_factor(VolumeNormalization::Track, Some(-3.5), None, None, None);
        assert!((factor - 10f32.powf(-3.5 / 20.0)).abs() < 0.001);
        assert_eq!(
            replay_gain_factor(VolumeNormalization::Track, Some(0.0), None, None, None),
            1.0
        );
        let boost = replay_gain_factor(VolumeNormalization::Track, Some(6.0), None, None, None);
        assert!((boost - 10f32.powf(6.0 / 20.0)).abs() < 0.001);
    }

    #[test]
    fn replay_gain_album_mode_uses_album_values() {
        let factor = replay_gain_factor(VolumeNormalization::Album, None, None, Some(-2.0), None);
        assert!((factor - 10f32.powf(-2.0 / 20.0)).abs() < 0.001);
        // Track 模式下 album 增益不生效。
        assert_eq!(
            replay_gain_factor(VolumeNormalization::Track, None, None, Some(-2.0), None),
            1.0
        );
    }

    #[test]
    fn replay_gain_off_ignores_metadata_and_prevents_clipping() {
        assert_eq!(
            replay_gain_factor(VolumeNormalization::Off, Some(-8.0), None, None, None),
            1.0
        );
        // 缺失增益数据时安全回退为 1.0。
        assert_eq!(
            replay_gain_factor(VolumeNormalization::Track, None, Some(0.9), None, None),
            1.0
        );
        // 峰值防削波：增益被限制在 1/peak。
        let clamped = replay_gain_factor(
            VolumeNormalization::Track,
            Some(10.0),
            Some(0.8),
            None,
            None,
        );
        assert!((clamped - 1.25).abs() < 0.001);
        // 结果始终落在安全范围内。
        assert_eq!(
            replay_gain_factor(VolumeNormalization::Track, Some(100.0), None, None, None),
            4.0
        );
    }

    #[test]
    fn measured_loudness_maps_rms_to_gain() {
        // 目标 -16 dBFS：rms -12 → 增益 -4 dB ≈ 0.631；rms -24 → +8 dB ≈ 2.512
        let loud = loudness_gain_factor(-12.0, 0.0);
        assert!((loud - 10f32.powf(-4.0 / 20.0)).abs() < 0.001);
        let quiet = loudness_gain_factor(-24.0, 0.0);
        assert!((quiet - 10f32.powf(8.0 / 20.0)).abs() < 0.001);
        // 峰值防削波：峰值 0.9 时增益不超过 1/0.9。
        let clamped = loudness_gain_factor(-24.0, 0.9);
        assert!((clamped - (1.0 / 0.9)).abs() < 0.001);
    }

    #[test]
    fn playback_mode_defaults_to_sequential_and_cycles_in_order() {
        assert_eq!(PlaybackMode::default(), PlaybackMode::Sequential);
        assert_eq!(PlaybackMode::Sequential.next(), PlaybackMode::RepeatAll);
        assert_eq!(PlaybackMode::RepeatAll.next(), PlaybackMode::RepeatOne);
        assert_eq!(PlaybackMode::RepeatOne.next(), PlaybackMode::Shuffle);
        assert_eq!(PlaybackMode::Shuffle.next(), PlaybackMode::Sequential);
    }

    #[test]
    fn shuffle_previous_returns_actually_played_tracks() {
        let mut history = ShuffleHistory::default();
        history.start(2);
        history.advance(5);
        history.advance(0);
        assert_eq!(history.previous(), Some(5)); // 回到真正播放过的上一首
        assert_eq!(history.previous(), Some(2));
        assert_eq!(history.previous(), None); // 无更多历史时停在当前
    }

    #[test]
    fn shuffle_restores_forward_after_going_back() {
        let mut history = ShuffleHistory::default();
        history.start(2);
        history.advance(5);
        history.advance(0);
        assert_eq!(history.previous(), Some(5));
        assert_eq!(history.restore_forward(), Some(0)); // 前进恢复刚才回退的歌
        assert_eq!(history.restore_forward(), None);
    }

    #[test]
    fn shuffle_next_excludes_recently_played_tracks() {
        let mut history = ShuffleHistory::default();
        history.start(0);
        for index in 1..=6 {
            history.advance(index);
        }
        let recent: Vec<usize> = history.recent(4).collect();
        assert_eq!(recent, vec![6, 5, 4, 3]); // 最近 4 首被排除
    }

    #[test]
    fn album_pagination_offsets_are_contiguous() {
        assert_eq!(album_page_offset(0, 100), 0);
        assert_eq!(album_page_offset(1, 100), 100);
        assert_eq!(album_page_offset(2, 100), 200);
        assert_eq!(album_page_offset(3, 50), 150);
    }

    #[test]
    fn shortcut_mapping_covers_playback_controls() {
        let none = Modifiers::default();
        assert_eq!(match_shortcut("space", none), Shortcut::TogglePlayback);
        assert_eq!(match_shortcut("left", none), Shortcut::SeekBack);
        assert_eq!(match_shortcut("right", none), Shortcut::SeekForward);
        assert_eq!(match_shortcut("up", none), Shortcut::VolumeUp);
        assert_eq!(match_shortcut("down", none), Shortcut::VolumeDown);
        assert_eq!(match_shortcut("m", none), Shortcut::ToggleMute);
        assert_eq!(match_shortcut("l", none), Shortcut::ToggleNowPlaying);
        assert_eq!(match_shortcut("q", none), Shortcut::ToggleQueue);
        assert_eq!(match_shortcut("escape", none), Shortcut::CloseOverlays);
        assert_eq!(match_shortcut("x", none), Shortcut::None);
    }

    #[test]
    fn shortcut_mapping_uses_ctrl_for_navigation_and_search() {
        let ctrl = Modifiers {
            control: true,
            ..Modifiers::default()
        };
        assert_eq!(match_shortcut("f", ctrl), Shortcut::FocusSearch);
        assert_eq!(match_shortcut("left", ctrl), Shortcut::Previous);
        assert_eq!(match_shortcut("right", ctrl), Shortcut::Next);
        assert_eq!(match_shortcut("space", ctrl), Shortcut::None);
    }

    #[test]
    fn seek_target_clamps_to_track_bounds() {
        let duration = Some(Duration::from_secs(180));
        assert_eq!(
            seek_target(Duration::from_secs(3), duration, -5),
            Duration::ZERO
        );
        assert_eq!(
            seek_target(Duration::from_secs(178), duration, 5),
            Duration::from_secs(180)
        );
        assert_eq!(
            seek_target(Duration::from_secs(60), duration, 5),
            Duration::from_secs(65)
        );
        assert_eq!(
            seek_target(Duration::from_secs(60), duration, -5),
            Duration::from_secs(55)
        );
    }
}
