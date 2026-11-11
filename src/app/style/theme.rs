//! Maps design tokens onto the component kit's theme configuration.

use std::rc::Rc;

use gpui_kit::component::{Theme, ThemeConfig};
use gpui_kit::{App, SharedString};

use super::tokens::{PaletteHex, DARK, LIGHT};

const CONTROL_RADIUS: usize = 8;
const LARGE_RADIUS: usize = 20;

fn hex(value: u32) -> Option<SharedString> {
    Some(format!("#{value:06X}").into())
}

fn apply_palette(config: &mut ThemeConfig, p: &PaletteHex) {
    config.radius = Some(CONTROL_RADIUS);
    config.radius_lg = Some(LARGE_RADIUS);
    let c = &mut config.colors;
    c.background = hex(p.page);
    c.foreground = hex(p.text);
    c.popover = hex(p.raised);
    c.popover_foreground = hex(p.text);
    c.primary = hex(p.brand);
    c.primary_hover = hex(p.brand_hover);
    c.primary_active = hex(p.brand_active);
    c.primary_foreground = hex(p.on_brand);
    c.button_primary = hex(p.brand);
    c.button_primary_hover = hex(p.brand_hover);
    c.button_primary_active = hex(p.brand_active);
    c.button_primary_foreground = hex(p.on_brand);
    c.secondary = hex(p.sunken);
    c.secondary_hover = hex(p.border);
    c.secondary_active = hex(p.border);
    c.secondary_foreground = hex(p.text);
    c.muted = hex(p.sunken);
    c.muted_foreground = hex(p.text_secondary);
    c.accent = hex(p.sunken);
    c.accent_foreground = hex(p.text);
    c.border = hex(p.border);
    c.input = hex(p.border);
    c.ring = hex(p.brand);
    c.caret = hex(p.brand);
    // The kit caps selection opacity at 30%; a soft surface tint disappears.
    c.selection = hex(p.brand);
    c.list_active = hex(p.brand_soft);
    c.list_active_border = hex(p.brand);
    c.list_hover = hex(p.sunken);
    c.sidebar = hex(p.page);
    c.sidebar_border = hex(p.page);
    c.sidebar_foreground = hex(p.text);
    c.sidebar_accent = hex(p.brand_soft);
    c.sidebar_accent_foreground = hex(p.brand);
    c.title_bar = hex(p.page);
    c.title_bar_border = hex(p.page);
    c.skeleton = hex(p.sunken);
    c.slider_bar = hex(p.brand);
    c.progress_bar = hex(p.brand);
    c.switch = hex(p.border);
    c.link = hex(p.brand);
    c.link_hover = hex(p.brand_hover);
    c.link_active = hex(p.brand_active);
}

/// `Theme::change` reuses these config copies, so switching light and dark at
/// runtime keeps the overrides.
pub(crate) fn apply_app_theme(cx: &mut App) {
    let mode = {
        let theme = Theme::global_mut(cx);
        let mut light = (*theme.light_theme).clone();
        apply_palette(&mut light, &LIGHT);
        theme.light_theme = Rc::new(light);

        let mut dark = (*theme.dark_theme).clone();
        apply_palette(&mut dark, &DARK);
        theme.dark_theme = Rc::new(dark);
        theme.mode
    };
    Theme::change(mode, None, cx);
}
