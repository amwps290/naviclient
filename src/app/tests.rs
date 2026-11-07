//! Application integration tests for navigation and global theme adjustments.

use gpui_kit::component::{ActiveTheme, Theme, ThemeMode};
use gpui_kit::{px, Hsla};

use crate::app::style::tokens::{palette, DARK, LIGHT};

fn close(a: Hsla, b: Hsla) -> bool {
    let (a, b) = (a.to_rgb(), b.to_rgb());
    (a.r - b.r).abs() < 0.01 && (a.g - b.g).abs() < 0.01 && (a.b - b.b).abs() < 0.01
}

#[gpui_pre::test]
fn theme_overrides_follow_runtime_mode_changes(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        Theme::change(ThemeMode::Light, None, cx);
        crate::app::apply_app_theme(cx);

        let light = LIGHT.resolve();
        assert!(close(cx.theme().background, light.page));
        assert!(close(cx.theme().primary, light.brand));
        assert!(close(cx.theme().muted_foreground, light.text_secondary));
        // Capsule buttons use the kit's solid secondary variant.
        assert!(close(cx.theme().button_secondary, light.sunken));
        assert!(close(cx.theme().button_secondary_foreground, light.text));
        assert!(close(cx.theme().button_primary, light.brand));
        assert!(close(cx.theme().button_primary_foreground, light.on_brand));
        assert!(close(palette(cx).page, light.page));
        assert_eq!(cx.theme().radius, px(8.0));
        assert_eq!(cx.theme().radius_lg, px(20.0));

        Theme::change(ThemeMode::Dark, None, cx);
        let dark = DARK.resolve();
        assert!(close(cx.theme().background, dark.page));
        assert!(close(cx.theme().primary, dark.brand));
        assert!(close(palette(cx).page, dark.page));
        assert_eq!(cx.theme().radius, px(8.0));

        Theme::change(ThemeMode::Light, None, cx);
        assert!(close(cx.theme().background, light.page));
        assert!(close(palette(cx).brand, light.brand));
    });
}

#[gpui_pre::test]
fn applying_theme_twice_changes_nothing(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        Theme::change(ThemeMode::Light, None, cx);
        crate::app::apply_app_theme(cx);
        let first = cx.theme().primary;
        crate::app::apply_app_theme(cx);
        assert_eq!(cx.theme().primary, first);
    });
}
