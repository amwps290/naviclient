//! Design tokens: page code reads colors and visual sizes from here.

use gpui_kit::component::ActiveTheme;
use gpui_kit::{rgb, App, Hsla};

/// Theme overrides need CSS strings and pages need `Hsla`, so hex is the source.
#[derive(Clone, Copy, Debug)]
pub(in crate::app) struct PaletteHex {
    pub(in crate::app) page: u32,
    pub(in crate::app) raised: u32,
    pub(in crate::app) sunken: u32,
    pub(in crate::app) text: u32,
    pub(in crate::app) text_secondary: u32,
    pub(in crate::app) text_muted: u32,
    pub(in crate::app) brand: u32,
    pub(in crate::app) brand_hover: u32,
    pub(in crate::app) brand_active: u32,
    pub(in crate::app) brand_soft: u32,
    pub(in crate::app) brand_soft_hover: u32,
    pub(in crate::app) on_brand: u32,
    pub(in crate::app) button_surface: u32,
    pub(in crate::app) button_surface_hover: u32,
    pub(in crate::app) button_surface_active: u32,
    pub(in crate::app) button_surface_border: u32,
    pub(in crate::app) button_tonal: u32,
    pub(in crate::app) button_tonal_hover: u32,
    pub(in crate::app) button_tonal_active: u32,
    pub(in crate::app) ghost_hover: u32,
    pub(in crate::app) ghost_active: u32,
    pub(in crate::app) danger: u32,
    pub(in crate::app) danger_hover: u32,
    pub(in crate::app) danger_active: u32,
    pub(in crate::app) on_danger: u32,
    pub(in crate::app) border: u32,
    pub(in crate::app) online: u32,
}

pub(in crate::app) const LIGHT: PaletteHex = PaletteHex {
    page: 0xF7F3EE,
    raised: 0xFFFFFF,
    sunken: 0xEFE9E2,
    text: 0x1C1917,
    text_secondary: 0x57534E,
    text_muted: 0x6F6863,
    brand: 0xB53660,
    brand_hover: 0x9E2D53,
    brand_active: 0x882546,
    brand_soft: 0xF2E4E3,
    brand_soft_hover: 0xECD9D8,
    on_brand: 0xFFFFFF,
    button_surface: 0xFFFFFF,
    button_surface_hover: 0xFAF6F1,
    button_surface_active: 0xEFE8E0,
    button_surface_border: 0xDFD6CA,
    button_tonal: 0xEAE3DA,
    button_tonal_hover: 0xE0D6CA,
    button_tonal_active: 0xD5C9BB,
    ghost_hover: 0xEFE9E2,
    ghost_active: 0xE4DCD2,
    danger: 0xC9353F,
    danger_hover: 0xB32C35,
    danger_active: 0x9B242C,
    on_danger: 0xFFFFFF,
    border: 0xE7E0D8,
    online: 0x2F7D4F,
};

pub(in crate::app) const DARK: PaletteHex = PaletteHex {
    page: 0x1C1917,
    raised: 0x292524,
    sunken: 0x24201E,
    text: 0xF5F5F4,
    text_secondary: 0xD6D3D1,
    text_muted: 0xA8A29E,
    brand: 0xF28AA8,
    brand_hover: 0xF5A0B8,
    brand_active: 0xF7B3C6,
    brand_soft: 0x3A292B,
    brand_soft_hover: 0x483135,
    on_brand: 0x1C1917,
    button_surface: 0x2E2927,
    button_surface_hover: 0x38322F,
    button_surface_active: 0x25211F,
    button_surface_border: 0x463F3B,
    button_tonal: 0x342F2C,
    button_tonal_hover: 0x403A36,
    button_tonal_active: 0x4C4540,
    ghost_hover: 0x433D39,
    ghost_active: 0x4E4742,
    danger: 0xF27D83,
    danger_hover: 0xF59499,
    danger_active: 0xF8AAAC,
    on_danger: 0x1C1917,
    border: 0x3A3431,
    online: 0x6FCF9F,
};

#[derive(Clone, Copy, Debug)]
pub(in crate::app) struct Palette {
    pub(in crate::app) page: Hsla,
    pub(in crate::app) raised: Hsla,
    pub(in crate::app) sunken: Hsla,
    pub(in crate::app) text: Hsla,
    pub(in crate::app) text_secondary: Hsla,
    pub(in crate::app) text_muted: Hsla,
    pub(in crate::app) brand: Hsla,
    pub(in crate::app) brand_hover: Hsla,
    pub(in crate::app) brand_soft: Hsla,
    pub(in crate::app) brand_soft_hover: Hsla,
    pub(in crate::app) on_brand: Hsla,
    pub(in crate::app) ghost_hover: Hsla,
    pub(in crate::app) ghost_active: Hsla,
    pub(in crate::app) border: Hsla,
    pub(in crate::app) online: Hsla,
}

impl PaletteHex {
    pub(in crate::app) fn resolve(&self) -> Palette {
        let color = |hex: u32| -> Hsla { rgb(hex).into() };
        Palette {
            page: color(self.page),
            raised: color(self.raised),
            sunken: color(self.sunken),
            text: color(self.text),
            text_secondary: color(self.text_secondary),
            text_muted: color(self.text_muted),
            brand: color(self.brand),
            brand_hover: color(self.brand_hover),
            brand_soft: color(self.brand_soft),
            brand_soft_hover: color(self.brand_soft_hover),
            on_brand: color(self.on_brand),
            ghost_hover: color(self.ghost_hover),
            ghost_active: color(self.ghost_active),
            border: color(self.border),
            online: color(self.online),
        }
    }
}

pub(in crate::app) fn palette(cx: &App) -> Palette {
    if cx.theme().is_dark() {
        DARK.resolve()
    } else {
        LIGHT.resolve()
    }
}

// Type scale.
pub(in crate::app) const PAGE_TITLE: f32 = 32.0;
pub(in crate::app) const META: f32 = 13.0;
pub(in crate::app) const SECTION_TITLE: f32 = 18.0;
pub(in crate::app) const BODY: f32 = 14.0;
pub(in crate::app) const CAPTION: f32 = 12.0;

pub(in crate::app) mod radius {
    pub(in crate::app) const CONTROL: f32 = 8.0;
    pub(in crate::app) const BUTTON: f32 = 10.0;
    pub(in crate::app) const ROW: f32 = 10.0;
    pub(in crate::app) const COVER: f32 = 12.0;
    pub(in crate::app) const LARGE: f32 = 20.0;
    pub(in crate::app) const FULL: f32 = 999.0;
}

// Icon button diameters.
pub(in crate::app) const ICON_BUTTON_LG: f32 = 36.0;
pub(in crate::app) const ICON_BUTTON_MD: f32 = 32.0;
pub(in crate::app) const ICON_BUTTON_SM: f32 = 30.0;
pub(in crate::app) const ICON_BUTTON_XS: f32 = 28.0;

// Window frame.
pub(in crate::app) const SIDEBAR_WIDTH: f32 = 200.0;
pub(in crate::app) const SIDEBAR_COLLAPSED_WIDTH: f32 = 56.0;
pub(in crate::app) const QUEUE_DRAWER_WIDTH: f32 = 380.0;
pub(in crate::app) const QUEUE_DRAWER_COMPACT_WIDTH: f32 = 320.0;
pub(in crate::app) const QUEUE_DRAWER_MARGIN: f32 = 12.0;
pub(in crate::app) const PLAYER_HEIGHT: f32 = 72.0;
pub(in crate::app) const PLAYER_COMPACT_HEIGHT: f32 = 64.0;
pub(in crate::app) const PLAYER_MARGIN: f32 = 12.0;
pub(in crate::app) const PLAYER_COMPACT_MARGIN: f32 = 8.0;
pub(in crate::app) const PLAY_BUTTON: f32 = 40.0;
pub(in crate::app) const PLAY_BUTTON_COMPACT: f32 = 36.0;
pub(in crate::app) const MINI_PLAY_BACKGROUND: u32 = 0x443B4D;
pub(in crate::app) const PAGE_PADDING: f32 = 32.0;
pub(in crate::app) const PAGE_PADDING_COMPACT: f32 = 20.0;

// Home and cards.
pub(in crate::app) const SECTION_GAP: f32 = 40.0;
pub(in crate::app) const CARD_TITLE: f32 = 26.0;
pub(in crate::app) const CARD_TITLE_COMPACT: f32 = 22.0;
pub(in crate::app) const HERO_CARD_HEIGHT: f32 = 176.0;
pub(in crate::app) const HERO_CARD_COMPACT_HEIGHT: f32 = 144.0;
pub(in crate::app) const HOME_FEATURE_PAIR_MIN_WIDTH: f32 = 640.0;
pub(in crate::app) const HERO_COVER: f32 = 128.0;
pub(in crate::app) const HERO_COVER_COMPACT: f32 = 96.0;
pub(in crate::app) const VIDEO_HERO_COVER: f32 = 168.0;
pub(in crate::app) const VIDEO_HERO_COVER_COMPACT: f32 = 124.0;
pub(in crate::app) const HOVER_PLAY_BUTTON: f32 = 40.0;

// Search.
pub(in crate::app) const SEARCH_CONTROL_HEIGHT: f32 = 44.0;

// Detail pages.
pub(in crate::app) const DETAIL_TITLE: f32 = 40.0;
pub(in crate::app) const DETAIL_TITLE_COMPACT: f32 = 28.0;
pub(in crate::app) const DETAIL_COVER: f32 = 220.0;
pub(in crate::app) const DETAIL_COVER_COMPACT: f32 = 140.0;
pub(in crate::app) const DETAIL_VIDEO_COVER: f32 = 264.0;
pub(in crate::app) const DETAIL_VIDEO_COVER_COMPACT: f32 = 192.0;
pub(in crate::app) const DETAIL_ARTIST_COVER: f32 = 150.0;
pub(in crate::app) const DETAIL_ARTIST_COVER_COMPACT: f32 = 110.0;
pub(in crate::app) const DETAIL_PLAY_BUTTON: f32 = 48.0;
pub(in crate::app) const PLAYLIST_STATUS_HEIGHT: f32 = 32.0;
pub(in crate::app) const DETAIL_COMPACT_WIDTH: f32 = 1040.0;
pub(in crate::app) const DETAIL_COMPACT_HEIGHT: f32 = 760.0;

// Now playing.
pub(in crate::app) const NOW_PLAYING_TITLE: f32 = 30.0;
pub(in crate::app) const LYRIC_ACTIVE: f32 = 28.0;
pub(in crate::app) const LYRIC: f32 = 20.0;
pub(in crate::app) const LYRIC_TIMING_STATUS_WIDTH: f32 = 112.0;
pub(in crate::app) const LYRIC_TIMING_HINT_HEIGHT: f32 = 20.0;

/// Fixed black dim: darkens the page behind modal overlays in both modes.
pub(in crate::app) fn scrim(cx: &App) -> Hsla {
    let alpha = if cx.theme().is_dark() { 0.44 } else { 0.24 };
    gpui_kit::hsla(0.0, 0.0, 0.0, alpha)
}

#[cfg(test)]
mod tests;
