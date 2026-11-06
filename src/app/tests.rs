//! Application integration tests for navigation and global theme adjustments.

use gpui_kit::{hsla, px};

use crate::app::covers::artwork::contrast_ratio;

#[gpui_pre::test]
fn light_theme_hint_contrast_is_darkened_for_readability(cx: &mut gpui_kit::TestAppContext) {
    cx.update(gpui_kit::init);
    cx.update(|cx| {
        use gpui_kit::component::{ActiveTheme, Theme, ThemeMode};

        // 基线：框架默认亮色主题的提示灰（neutral-500）在小字号下不足。
        Theme::change(ThemeMode::Light, None, cx);
        let white = hsla(0.0, 0.0, 1.0, 1.0);
        let before = contrast_ratio(cx.theme().colors.muted_foreground, white);
        assert!(
            (4.0..5.0).contains(&before),
            "基线应为 neutral-500（约 4.7:1），实际 {before}"
        );

        // 修正后：亮色提示灰加深到 neutral-600（约 7.4:1），小字号也清晰。
        crate::app::adjust_light_theme_contrast(cx);
        let after = contrast_ratio(cx.theme().colors.muted_foreground, white);
        assert!(
            (7.0..8.0).contains(&after),
            "修正后应为 neutral-600（约 7.4:1），实际 {after}"
        );
        assert!(after > before);
        assert_eq!(cx.theme().radius, px(8.0));

        // 幂等：重复调用（如后续启动路径复用）不改变结果。
        crate::app::adjust_light_theme_contrast(cx);
        assert_eq!(
            contrast_ratio(cx.theme().colors.muted_foreground, white),
            after
        );

        // 深色主题不受影响：切换到 Dark 后提示灰是另一套值。
        Theme::change(ThemeMode::Dark, None, cx);
        assert_eq!(cx.theme().radius, px(8.0));
        assert_ne!(
            contrast_ratio(cx.theme().colors.muted_foreground, white),
            after
        );
    });
}
