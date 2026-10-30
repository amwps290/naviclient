//! 应用层纯逻辑：快捷键映射、队列索引计算、封面调色板提取、
//! 增益/对比度/格式化等无副作用的函数与小型数据类型。

use rust_i18n::t;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{
    hsla, px, Hsla, Image as GpuiImage, ImageFormat as GpuiImageFormat, Keystroke, Modifiers,
    Pixels,
};

use crate::audio::{PlaybackState, StreamFailure};
use crate::models::{
    Artist, ArtistSort, PlayModes, RepeatMode, ShortcutAction, Song, TranscodingQuality,
    VolumeNormalization,
};
use crate::msg::DecodedCover;

// The mini player content target size. On Linux, the client-side window shadow
// (gpui-component's WindowBorder) pads each edge of the window, so the actual
// drawable area is smaller than the outer window size. enter_mini_mode adds the
// window paddings to keep the content at this size on every platform.
// 260x56 leaves the title/artist column ~110px wide; the old 200x50 left ~70px,
// which forced the scrolling text to run almost constantly.
pub(super) const MINI_WINDOW_WIDTH: Pixels = px(260.0);
pub(super) const MINI_WINDOW_HEIGHT: Pixels = px(56.0);
pub(super) const ALBUM_PAGE_SIZE: usize = 100;
/// 歌曲行高：60px 内容 + 1px 顶部边框。uniform_list 限高模式和封面预取都依赖它。
pub(super) const SONG_ROW_HEIGHT: f32 = 61.0;

pub(super) fn compact_window_layout(viewport_width: f32) -> bool {
    viewport_width < 1_040.0
}

pub(super) fn compact_song_columns(content_width: f32) -> bool {
    content_width > 0.0 && content_width < 900.0
}

pub(super) fn item_instance_key(scope: &str, kind: &str, index: usize, id: &str) -> String {
    format!("{scope}/{kind}/{index}/{id}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum View {
    Home,
    Favorites,
    Artists,
    Albums,
    Genres,
    GenreDetail,
    Playlists,
    Search,
    ArtistDetail,
    AlbumDetail,
    PlaylistDetail,
    NowPlaying,
}

impl View {
    /// 侧边栏入口的稳定元素 ID 前缀。与显示文案解耦，切换界面语言时
    /// 元素 ID 不变（否则 GPUI 会把整行当作新元素、丢掉悬停/焦点状态）。
    pub(super) fn nav_id(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Favorites => "favorites",
            Self::Artists | Self::ArtistDetail => "artists",
            Self::Albums | Self::AlbumDetail => "albums",
            Self::Genres | Self::GenreDetail => "genres",
            Self::Playlists | Self::PlaylistDetail => "playlists",
            Self::Search => "search",
            Self::NowPlaying => "now-playing",
        }
    }
}

/// 搜索结果的显示过滤；会话内状态，不持久化。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SearchFilter {
    #[default]
    All,
    Artists,
    Albums,
    Songs,
}

impl SearchFilter {
    pub(super) fn label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::All => t!("All"),
            Self::Artists => t!("Artists"),
            Self::Albums => t!("Albums"),
            Self::Songs => t!("Songs"),
        }
    }

    pub(super) const ALL: [Self; 4] = [Self::All, Self::Artists, Self::Albums, Self::Songs];
}

/// 专辑内歌曲按（碟号， 曲目号）排序；缺失字段排到最后，
/// 同一曲内缺失曲目号的按标题保持稳定顺序。
pub(super) fn sort_album_songs(songs: &mut [Song]) {
    songs.sort_by(|a, b| {
        let disc = |song: &Song| song.disc.unwrap_or(i32::MAX);
        let track = |song: &Song| song.track.unwrap_or(i32::MAX);
        disc(a)
            .cmp(&disc(b))
            .then_with(|| track(a).cmp(&track(b)))
            .then_with(|| a.title.cmp(&b.title))
    });
}

#[derive(Debug, Default)]
pub(super) struct ShuffleHistory {
    pub(super) played: Vec<usize>,
    pub(super) forward: Vec<usize>,
}

impl ShuffleHistory {
    pub(super) fn start(&mut self, index: usize) {
        self.played = vec![index];
        self.forward.clear();
    }

    /// 前进到 next：丢弃回退暂存，记录已播放序列。
    pub(super) fn advance(&mut self, next: usize) {
        self.forward.clear();
        self.played.push(next);
    }

    /// 回退到上一首：返回真正播放过的上一首；无可回退时返回 None。
    pub(super) fn previous(&mut self) -> Option<usize> {
        if self.played.len() < 2 {
            return None;
        }
        let current = self.played.pop()?;
        self.forward.push(current);
        self.played.last().copied()
    }

    /// 回退后恢复前进：返回暂存的下一首；无暂存时返回 None。
    pub(super) fn restore_forward(&mut self) -> Option<usize> {
        let index = self.forward.pop()?;
        self.played.push(index);
        Some(index)
    }

    /// 最近播放过的 index（用于随机排除，避免短时重复）。
    pub(super) fn recent(&self, max: usize) -> impl Iterator<Item = usize> + '_ {
        self.played.iter().rev().take(max).copied()
    }

    /// 从持久化快照恢复。
    pub(super) fn restore(played: Vec<usize>, forward: Vec<usize>) -> Self {
        Self { played, forward }
    }

    /// 导出快照用于持久化。
    pub(super) fn snapshot(&self) -> (Vec<usize>, Vec<usize>) {
        (self.played.clone(), self.forward.clone())
    }

    pub(super) fn remap(&mut self, mut map: impl FnMut(usize) -> Option<usize>) {
        self.played = self.played.iter().copied().filter_map(&mut map).collect();
        self.forward = self.forward.iter().copied().filter_map(map).collect();
    }
}

#[derive(Clone, Copy)]
pub(super) struct NextPreload {
    pub(super) id: u64,
    pub(super) index: usize,
    pub(super) quality: TranscodingQuality,
}

/// 一次自动重连的决策结果。
pub(super) enum ReconnectDecision {
    /// 这类失败重试也无用（解码 / 本地输出 / 4xx）。
    DoNotRetry,
    /// 重试次数已用尽。
    Exhausted,
    /// 等 `delay` 后从断点重试，这是第 `attempt` 次。
    Retry { attempt: u8, delay: Duration },
}

/// 根据之前的重试计数和失败类型决定下一步。纯函数，方便回归测试。
pub(super) fn reconnect_decision(
    failure: Option<StreamFailure>,
    previous_attempts: u8,
    max_attempts: u8,
) -> ReconnectDecision {
    if !failure.is_some_and(StreamFailure::is_retryable) {
        return ReconnectDecision::DoNotRetry;
    }
    let attempt = previous_attempts.saturating_add(1);
    if attempt > max_attempts {
        return ReconnectDecision::Exhausted;
    }
    // 退避：第 1 次 1.5s，之后 4s；与失败类型无关，保持可预测。
    let delay = match attempt {
        1 => Duration::from_millis(1500),
        _ => Duration::from_secs(4),
    };
    ReconnectDecision::Retry { attempt, delay }
}

pub(super) fn should_preload_next(playback: &PlaybackState) -> bool {
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

pub(super) fn next_queue_index(
    modes: PlayModes,
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
    if modes.shuffle {
        // 随机 + 单曲循环：自然结束重播当前，手动切歌仍随机。
        if natural_end && modes.repeat == RepeatMode::One {
            return Some(current);
        }
        return shuffle_forward
            .or(reserved)
            .or_else(|| Some(random()))
            .filter(|i| *i < len);
    }
    match modes.repeat {
        RepeatMode::One if natural_end => Some(current),
        RepeatMode::One | RepeatMode::All => advance_index(current, len, true, true),
        RepeatMode::Off => advance_index(current, len, true, false),
    }
}

/// 按所选方式对艺术家列表原地排序。
/// 名称比较不区分大小写，与 Navidrome 索引分组的顺序一致；
/// 专辑数相同时回退到名称 A→Z，保证多次排序结果稳定。
pub(super) fn sort_artists(artists: &mut [Artist], sort: ArtistSort) {
    match sort {
        ArtistSort::Name => artists.sort_by_key(artist_name_key),
        ArtistSort::NameDesc => {
            artists.sort_by_key(|artist| std::cmp::Reverse(artist_name_key(artist)))
        }
        ArtistSort::AlbumCount => artists.sort_by(|a, b| {
            b.album_count
                .unwrap_or(0)
                .cmp(&a.album_count.unwrap_or(0))
                .then_with(|| artist_name_key(a).cmp(&artist_name_key(b)))
        }),
    }
}

fn artist_name_key(artist: &Artist) -> String {
    artist.name.trim().to_lowercase()
}

/// 计算顺序（非随机）切歌的目标 index；`wraps` 为 true 时队列首尾循环，否则在边界停止。
pub(super) fn advance_index(
    current: usize,
    len: usize,
    forward: bool,
    wraps: bool,
) -> Option<usize> {
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
pub(super) fn queue_index_after_remove(
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
pub(super) fn queue_index_after_move(from: usize, to: usize, current: usize) -> usize {
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
pub(super) fn should_scrobble(position: Duration, duration: Option<Duration>) -> bool {
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
pub(super) fn replay_gain_factor(
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
pub(super) const LOUDNESS_TARGET_DB: f32 = -16.0;

/// 依据本地测量的响度与峰值计算增益系数（1.0 = 不变）。
pub(super) fn loudness_gain_factor(measured_rms_db: f32, peak: f32) -> f32 {
    let gain_db = LOUDNESS_TARGET_DB - measured_rms_db;
    let mut factor = 10f32.powf(gain_db / 20.0);
    if peak > 0.0 && peak.is_finite() {
        factor = factor.min(1.0 / peak);
    }
    factor.clamp(0.1, 4.0)
}

/// 计算专辑分页的 offset；相邻页之间连续、不重叠。
pub(super) fn album_page_offset(page: usize, page_size: usize) -> u32 {
    (page as u32) * (page_size as u32)
}

/// 按用户配置匹配快捷键动作。配置缺少新增动作时由 `binding` 回退默认值。
pub(super) fn match_shortcut(
    settings: &crate::models::ShortcutSettings,
    keystroke: &Keystroke,
) -> Option<ShortcutAction> {
    ShortcutAction::ALL.into_iter().find(|action| {
        Keystroke::parse(settings.binding(*action)).is_ok_and(|binding| {
            binding.key == keystroke.key && binding.modifiers == keystroke.modifiers
        })
    })
}

pub(super) fn shortcut_label(action: ShortcutAction) -> std::borrow::Cow<'static, str> {
    match action {
        ShortcutAction::TogglePlayback => t!("Play / pause"),
        ShortcutAction::Previous => t!("Previous track"),
        ShortcutAction::Next => t!("Next track"),
        ShortcutAction::SeekBack => t!("Seek backward 5 seconds"),
        ShortcutAction::SeekForward => t!("Seek forward 5 seconds"),
        ShortcutAction::VolumeUp => t!("Volume up"),
        ShortcutAction::VolumeDown => t!("Volume down"),
        ShortcutAction::ToggleMute => t!("Mute / unmute"),
        ShortcutAction::ToggleNowPlaying => t!("Open / close now playing"),
        ShortcutAction::ToggleQueue => t!("Open / close queue"),
        ShortcutAction::FocusSearch => t!("Focus search"),
        ShortcutAction::CloseOverlays => t!("Close overlay / go back"),
        ShortcutAction::SelectPrevious => t!("Select previous song"),
        ShortcutAction::SelectNext => t!("Select next song"),
        ShortcutAction::SelectAll => t!("Select all songs"),
        ShortcutAction::PlaySelected => t!("Play selected song"),
    }
}

pub(super) fn shortcut_display(binding: &str) -> String {
    Keystroke::parse(binding)
        .map(|keystroke| keystroke.to_string())
        .unwrap_or_else(|_| binding.to_string())
}

pub(super) fn shortcut_conflict(
    settings: &crate::models::ShortcutSettings,
    target: ShortcutAction,
    binding: &str,
) -> Option<ShortcutAction> {
    let normalized = Keystroke::parse(binding).ok()?.unparse();
    ShortcutAction::ALL.into_iter().find(|action| {
        *action != target
            && Keystroke::parse(settings.binding(*action))
                .is_ok_and(|candidate| candidate.unparse() == normalized)
    })
}

pub(super) fn global_shortcut_conflict(
    settings: &crate::models::GlobalShortcutSettings,
    target: ShortcutAction,
    binding: &str,
) -> Option<ShortcutAction> {
    let normalized = Keystroke::parse(binding).ok()?.unparse();
    ShortcutAction::GLOBAL_PLAYBACK.into_iter().find(|action| {
        *action != target
            && Keystroke::parse(settings.binding(*action))
                .is_ok_and(|candidate| candidate.unparse() == normalized)
    })
}

/// Shift 范围选择锚点的优先级：已有同作用域多选的锚点（允许连续
/// 扩展）> 键盘光标 > 本次目标行。
pub(super) fn range_anchor(
    multi_anchor: Option<usize>,
    cursor: Option<usize>,
    fallback: usize,
) -> usize {
    multi_anchor.or(cursor).unwrap_or(fallback)
}

/// Shift+点击的范围选择集合（锚点到点击行的闭区间，升序）。
pub(super) fn selection_range(anchor: usize, clicked: usize) -> std::collections::BTreeSet<usize> {
    if anchor <= clicked {
        (anchor..=clicked).collect()
    } else {
        (clicked..=anchor).collect()
    }
}

/// 块插入后随机播放历史索引的平移：插入点及之后平移 count，之前的
/// 保持不变。与逐首插入（count = 1）的语义一致。
pub(super) fn history_index_after_insert(index: usize, insert_at: usize, count: usize) -> usize {
    if index >= insert_at {
        index + count
    } else {
        index
    }
}

/// 计算快进/快退的目标位置，不越过歌曲起点与终点。
pub(super) fn seek_target(
    position: Duration,
    duration: Option<Duration>,
    delta_secs: i64,
) -> Duration {
    let duration = duration.unwrap_or_default();
    if delta_secs >= 0 {
        (position + Duration::from_secs(delta_secs as u64)).min(duration)
    } else {
        position.saturating_sub(Duration::from_secs((-delta_secs) as u64))
    }
}

/// 在后台线程解码封面：按需提取调色板并构造 GPUI 图片对象，避免阻塞 UI 线程。
/// 封面是否需要发起请求：
/// - 已有图像且这次不需要调色板 → 不需要（网格/队列预取的常规情形）；
/// - 已有图像但调色板还没提取、这次又需要（播放该专辑）→ 需要（补提，
///   走封面磁盘缓存，不重新走网络）；
/// - 图像还没有 → 需要。
///
/// 调色板提取耗时明显，只有播放页/侧边栏配色等少数场景需要它；
/// 网格与队列预取始终用 `want_palette = false`。关键约束：一次
/// 无调色板的预取绝不能永久“烧掉”这个封面 id，否则该专辑永远
/// 拿不到动态配色（历史上正是这只 bug：`requested_covers` 只增不减）。
pub(super) fn cover_request_needed(
    have_image: bool,
    have_palette: bool,
    want_palette: bool,
) -> bool {
    !(have_image && (!want_palette || have_palette))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SongRowClickAction {
    Select,
    Play,
    ToggleMulti,
    RangeMulti,
}

/// 歌曲行遵循桌面媒体库语义：单击选择、双击播放；Ctrl/Shift 保留多选。
pub(super) fn song_row_click_action(
    modifiers: Modifiers,
    click_count: usize,
) -> SongRowClickAction {
    if modifiers.control && !modifiers.alt && !modifiers.platform {
        SongRowClickAction::ToggleMulti
    } else if modifiers.shift && !modifiers.alt && !modifiers.platform {
        SongRowClickAction::RangeMulti
    } else if click_count >= 2 {
        SongRowClickAction::Play
    } else {
        SongRowClickAction::Select
    }
}

/// 更新最近使用队列，返回因超限被淘汰的最旧键。
pub(super) fn touch_lru(lru: &mut VecDeque<u64>, key: u64, capacity: usize) -> Option<u64> {
    debug_assert!(capacity > 0);
    if let Some(index) = lru.iter().position(|candidate| *candidate == key) {
        lru.remove(index);
    }
    lru.push_front(key);
    (lru.len() > capacity).then(|| lru.pop_back()).flatten()
}

pub(super) fn decode_cover(bytes: Vec<u8>, want_palette: bool) -> DecodedCover {
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

pub(super) fn normalize_volume(volume: f32) -> f32 {
    if volume.is_finite() {
        volume.clamp(0.0, 1.0)
    } else {
        0.7
    }
}

pub(super) fn effective_volume(volume: f32, muted: bool) -> f32 {
    if muted {
        0.0
    } else {
        normalize_volume(volume)
    }
}

pub(super) fn restored_volume(volume_before_mute: f32) -> f32 {
    let volume = normalize_volume(volume_before_mute);
    if volume > 0.001 {
        volume
    } else {
        0.7
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PlaybackTechnicalInfo {
    pub(super) format: String,
    pub(super) tooltip: String,
}

pub(super) fn playback_technical_info(
    song: &Song,
    quality: TranscodingQuality,
) -> PlaybackTechnicalInfo {
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
            .unwrap_or_else(|| t!("Unknown size").to_string());
        let bit_rate = format!("{bit_rate} kbps");
        let display = t!(
            "MP3 · %{bit_rate} · %{size}",
            bit_rate = bit_rate,
            size = size
        )
        .to_string();
        return PlaybackTechnicalInfo {
            format: "MP3".to_string(),
            tooltip: t!(
                "Current stream: %{display}\nSource file: %{source}",
                display = display,
                source = source.display
            )
            .to_string(),
        };
    }

    PlaybackTechnicalInfo {
        format: source.format,
        tooltip: t!("Current stream: %{display}", display = source.display).to_string(),
    }
}

pub(super) struct SourceTechnicalInfo {
    format: String,
    display: String,
}

pub(super) fn source_technical_info(song: &Song) -> SourceTechnicalInfo {
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
        .unwrap_or_else(|| t!("Unknown rate").to_string());
    let size = song
        .size
        .and_then(|size| u64::try_from(size).ok())
        .map(format_file_size)
        .unwrap_or_else(|| t!("Unknown size").to_string());
    let display = format!("{format} · {bit_rate} · {size}");

    SourceTechnicalInfo { format, display }
}

pub(super) fn format_from_content_type(content_type: &str) -> Option<String> {
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

pub(super) fn estimated_bit_rate(song: &Song) -> Option<u64> {
    let bytes = u64::try_from(song.size?).ok()?;
    let seconds = u64::try_from(song.duration?).ok()?;
    (seconds > 0).then(|| bytes.saturating_mul(8) / seconds / 1_000)
}

pub(super) fn format_file_size(bytes: u64) -> String {
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

pub(super) fn relative_luminance(color: Hsla) -> f32 {
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

pub(super) fn contrast_ratio(a: Hsla, b: Hsla) -> f32 {
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

pub(super) fn rgb_to_hsla(red: f32, green: f32, blue: f32) -> Hsla {
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

pub(super) fn extract_cover_palette(bytes: &[u8]) -> Option<(Hsla, Hsla)> {
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

pub(super) fn gpui_image_format(format: image::ImageFormat) -> Option<GpuiImageFormat> {
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
