//! Soft cover-colored backgrounds for content surfaces. Cover colors are
//! normalized first, so any artwork keeps the text above it readable.

use gpui_kit::{hsla, linear_color_stop, linear_gradient, Background, Hsla};

const MAX_SATURATION: f32 = 0.6;
const LIGHT_LIGHTNESS: f32 = 0.62;
const DARK_LIGHTNESS: f32 = 0.42;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum TintSurface {
    HomeFeature,
    DetailHeader,
    NowPlaying,
    MiniPlayer,
}

impl TintSurface {
    fn strengths(self, dark: bool) -> (f32, f32) {
        match (self, dark) {
            (Self::HomeFeature, false) => (0.24, 0.18),
            (Self::HomeFeature, true) => (0.18, 0.14),
            (Self::DetailHeader, false) => (0.22, 0.22),
            (Self::DetailHeader, true) => (0.18, 0.18),
            (Self::NowPlaying, false) => (0.26, 0.20),
            (Self::NowPlaying, true) => (0.20, 0.16),
            (Self::MiniPlayer, false) => (0.30, 0.24),
            (Self::MiniPlayer, true) => (0.24, 0.20),
        }
    }
}

fn normalize(color: Hsla, dark: bool) -> Hsla {
    hsla(
        color.h,
        color.s.min(MAX_SATURATION),
        if dark {
            DARK_LIGHTNESS
        } else {
            LIGHT_LIGHTNESS
        },
        1.0,
    )
}

pub(in crate::app) fn cover_tint(
    surface: TintSurface,
    palette: Option<(Hsla, Hsla)>,
    page: Hsla,
    dark: bool,
) -> Option<(Hsla, Hsla)> {
    let (base, accent) = palette?;
    let (base_strength, accent_strength) = surface.strengths(dark);
    Some((
        page.blend(normalize(base, dark).opacity(base_strength)),
        page.blend(normalize(accent, dark).opacity(accent_strength)),
    ))
}

pub(in crate::app) fn tint_background(
    tint: Option<(Hsla, Hsla)>,
    fallback: Hsla,
    angle: f32,
) -> Background {
    match tint {
        Some((start, end)) => linear_gradient(
            angle,
            linear_color_stop(start, 0.0),
            linear_color_stop(end, 1.0),
        ),
        None => fallback.into(),
    }
}

#[cfg(test)]
mod tests;
