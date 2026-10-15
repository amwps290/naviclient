//! 应用层纯逻辑：快捷键映射、队列索引计算、封面调色板提取、
//! 增益/对比度/格式化等无副作用的函数与小型数据类型。

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{
    hsla, px, Hsla, Image as GpuiImage, ImageFormat as GpuiImageFormat, Modifiers, Pixels,
};

use crate::audio::PlaybackState;
use crate::models::{
    Artist, ArtistSort, PlaybackMode, Song, TranscodingQuality, VolumeNormalization,
};
use crate::msg::DecodedCover;

// The mini player content target size. On Linux, the client-side window shadow
// (gpui-component's WindowBorder) pads each edge of the window, so the actual
// drawable area is smaller than the outer window size. enter_mini_mode adds the
// window paddings to keep the content at this size on every platform.
pub(super) const MINI_WINDOW_WIDTH: Pixels = px(200.0);
pub(super) const MINI_WINDOW_HEIGHT: Pixels = px(50.0);
pub(super) const ALBUM_PAGE_SIZE: usize = 100;
/// 歌曲行高：60px 内容 + 1px 顶部边框。uniform_list 限高模式和封面预取都依赖它。
pub(super) const SONG_ROW_HEIGHT: f32 = 61.0;

pub(super) fn item_instance_key(scope: &str, kind: &str, index: usize, id: &str) -> String {
    format!("{scope}/{kind}/{index}/{id}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum View {
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
    pub(super) fn next(self) -> Self {
        match self {
            Self::Sequential => Self::RepeatAll,
            Self::RepeatAll => Self::RepeatOne,
            Self::RepeatOne => Self::Shuffle,
            Self::Shuffle => Self::Sequential,
        }
    }

    pub(super) fn tooltip(self) -> &'static str {
        match self {
            Self::Sequential => "Play sequentially",
            Self::RepeatAll => "Repeat all",
            Self::RepeatOne => "Repeat current track",
            Self::Shuffle => "Shuffle queue",
        }
    }
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

/// 键盘快捷键动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Shortcut {
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
    SelectPrevious,
    SelectNext,
    PlaySelected,
    None,
}

/// 将按键映射为快捷键动作（纯逻辑，便于测试）。
pub(super) fn match_shortcut(key: &str, mods: Modifiers) -> Shortcut {
    if mods.control && !mods.alt && !mods.shift && !mods.platform {
        match key {
            "f" => return Shortcut::FocusSearch,
            "left" => return Shortcut::Previous,
            "right" => return Shortcut::Next,
            _ => {}
        }
    }
    // Shift+↑/↓：歌曲列表键盘导航（与音量键让开）。
    if mods.shift && !mods.control && !mods.alt && !mods.platform {
        match key {
            "up" => return Shortcut::SelectPrevious,
            "down" => return Shortcut::SelectNext,
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
        "enter" => Shortcut::PlaySelected,
        "escape" => Shortcut::CloseOverlays,
        _ => Shortcut::None,
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
        .unwrap_or_else(|| "Unknown rate".to_string());
    let size = song
        .size
        .and_then(|size| u64::try_from(size).ok())
        .map(format_file_size)
        .unwrap_or_else(|| "Unknown size".to_string());
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
