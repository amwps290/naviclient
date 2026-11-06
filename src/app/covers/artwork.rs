//! Cover decoding and palette/color helpers.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{hsla, Hsla, Image as GpuiImage, ImageFormat as GpuiImageFormat};

use crate::msg::DecodedCover;

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
pub(in crate::app) fn cover_request_needed(
    have_image: bool,
    have_palette: bool,
    want_palette: bool,
) -> bool {
    !(have_image && (!want_palette || have_palette))
}

/// 在后台线程解码封面：按需提取调色板并构造 GPUI 图片对象，避免阻塞 UI 线程。
pub(in crate::app) fn decode_cover(bytes: Vec<u8>, want_palette: bool) -> DecodedCover {
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

pub(in crate::app) fn relative_luminance(color: Hsla) -> f32 {
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

pub(in crate::app) fn contrast_ratio(a: Hsla, b: Hsla) -> f32 {
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

pub(in crate::app) fn rgb_to_hsla(red: f32, green: f32, blue: f32) -> Hsla {
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

pub(in crate::app) fn extract_cover_palette(bytes: &[u8]) -> Option<(Hsla, Hsla)> {
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

pub(in crate::app) fn gpui_image_format(format: image::ImageFormat) -> Option<GpuiImageFormat> {
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
mod tests;
