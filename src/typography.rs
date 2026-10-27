//! Cross-platform UI font selection.
//!
//! GPUI's virtual `.SystemUIFont` resolves to the platform UI font. On
//! Windows that is normally Segoe UI, which has no CJK glyphs, so DirectWrite
//! chooses a different font for every Chinese run. The result depends on the
//! Windows locale and can mix incompatible metrics and weights in one label.
//! A music library also contains CJK metadata when the application language is
//! English, so the choice cannot be tied only to translated UI strings.

use gpui_kit::component::Theme;
use gpui_kit::{App, SharedString};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Platform {
    Windows,
    Macos,
    Linux,
    Other,
}

impl Platform {
    const fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else {
            Self::Other
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Macos => "macos",
            Self::Linux => "linux",
            Self::Other => "other",
        }
    }
}

const WINDOWS_SC_FONTS: &[&str] = &[
    "Microsoft YaHei UI",
    "Microsoft YaHei",
    "微软雅黑 UI",
    "微软雅黑",
    "DengXian",
    "等线",
    "Microsoft JhengHei UI",
    "Microsoft JhengHei",
    "微軟正黑體 UI",
    "微軟正黑體",
];

const WINDOWS_TC_FONTS: &[&str] = &[
    "Microsoft JhengHei UI",
    "Microsoft JhengHei",
    "微軟正黑體 UI",
    "微軟正黑體",
    "Microsoft YaHei UI",
    "Microsoft YaHei",
    "微软雅黑 UI",
    "微软雅黑",
];

const MACOS_SC_FONTS: &[&str] = &["PingFang SC", "Hiragino Sans GB", "Heiti SC", "PingFang TC"];

const MACOS_TC_FONTS: &[&str] = &["PingFang TC", "Heiti TC", "PingFang HK", "PingFang SC"];

const LINUX_SC_FONTS: &[&str] = &[
    "Noto Sans CJK SC",
    "Noto Sans SC",
    "Source Han Sans CN",
    "Source Han Sans SC",
    "Sarasa UI SC",
    "Sarasa Gothic SC",
    "WenQuanYi Micro Hei",
    "WenQuanYi Zen Hei",
    "Noto Sans CJK TC",
];

const LINUX_TC_FONTS: &[&str] = &[
    "Noto Sans CJK TC",
    "Noto Sans TC",
    "Source Han Sans TW",
    "Source Han Sans TC",
    "Sarasa UI TC",
    "Sarasa Gothic TC",
    "WenQuanYi Micro Hei",
    "WenQuanYi Zen Hei",
    "Noto Sans CJK SC",
];

const PORTABLE_SC_FONTS: &[&str] = &[
    "Noto Sans CJK SC",
    "Noto Sans SC",
    "Source Han Sans CN",
    "Source Han Sans SC",
];

const PORTABLE_TC_FONTS: &[&str] = &[
    "Noto Sans CJK TC",
    "Noto Sans TC",
    "Source Han Sans TW",
    "Source Han Sans TC",
];

fn prefers_traditional_chinese(locale: &str) -> bool {
    let locale = locale.trim().to_ascii_lowercase();
    locale.starts_with("zh-tw")
        || locale.starts_with("zh-hk")
        || locale.starts_with("zh-mo")
        || locale.contains("hant")
}

fn candidates(platform: Platform, locale: &str) -> &'static [&'static str] {
    match (platform, prefers_traditional_chinese(locale)) {
        (Platform::Windows, false) => WINDOWS_SC_FONTS,
        (Platform::Windows, true) => WINDOWS_TC_FONTS,
        (Platform::Macos, false) => MACOS_SC_FONTS,
        (Platform::Macos, true) => MACOS_TC_FONTS,
        (Platform::Linux, false) => LINUX_SC_FONTS,
        (Platform::Linux, true) => LINUX_TC_FONTS,
        (Platform::Other, false) => PORTABLE_SC_FONTS,
        (Platform::Other, true) => PORTABLE_TC_FONTS,
    }
}

fn select_ui_font(platform: Platform, locale: &str, installed: &[String]) -> Option<String> {
    candidates(platform, locale).iter().find_map(|candidate| {
        installed
            .iter()
            .find(|family| family.eq_ignore_ascii_case(candidate))
            .cloned()
    })
}

/// Select an installed CJK-capable UI family and apply it to the global theme.
///
/// Keeping the original family when no candidate is installed preserves
/// GPUI's safe platform fallback and avoids naming a font that cannot resolve.
pub fn configure_ui_font(cx: &mut App, locale: &str) -> Option<SharedString> {
    let platform = Platform::current();
    let installed = cx.text_system().all_font_names();
    let Some(family) = select_ui_font(platform, locale, &installed) else {
        log::warn!(
            "no preferred CJK UI font is installed; platform={} locale={locale}; keeping GPUI system font",
            platform.name()
        );
        return None;
    };

    let family = SharedString::from(family);
    if Theme::global(cx).font_family != family {
        Theme::update(cx, |theme| theme.font_family = family.clone());
    }
    log::info!(
        "selected UI font family={family:?}; platform={} locale={locale}",
        platform.name()
    );
    Some(family)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn windows_simplified_prefers_yahei_ui() {
        let fonts = installed(&["Microsoft JhengHei UI", "Microsoft YaHei UI"]);
        assert_eq!(
            select_ui_font(Platform::Windows, "zh-CN", &fonts).as_deref(),
            Some("Microsoft YaHei UI")
        );
    }

    #[test]
    fn windows_traditional_prefers_jhenghei_ui() {
        let fonts = installed(&["Microsoft YaHei UI", "Microsoft JhengHei UI"]);
        assert_eq!(
            select_ui_font(Platform::Windows, "zh-Hant", &fonts).as_deref(),
            Some("Microsoft JhengHei UI")
        );
    }

    #[test]
    fn english_ui_still_selects_a_cjk_font_for_music_metadata() {
        let fonts = installed(&["Segoe UI", "Microsoft YaHei UI"]);
        assert_eq!(
            select_ui_font(Platform::Windows, "en", &fonts).as_deref(),
            Some("Microsoft YaHei UI")
        );
    }

    #[test]
    fn linux_uses_known_cjk_family_in_priority_order() {
        let fonts = installed(&["WenQuanYi Micro Hei", "Noto Sans CJK SC"]);
        assert_eq!(
            select_ui_font(Platform::Linux, "zh-CN", &fonts).as_deref(),
            Some("Noto Sans CJK SC")
        );
    }

    #[test]
    fn selection_is_case_insensitive_and_preserves_installed_name() {
        let fonts = installed(&["noto sans cjk sc"]);
        assert_eq!(
            select_ui_font(Platform::Linux, "zh-CN", &fonts).as_deref(),
            Some("noto sans cjk sc")
        );
    }

    #[test]
    fn missing_candidates_keep_the_gpui_system_font() {
        let fonts = installed(&["DejaVu Sans", "Segoe UI"]);
        assert_eq!(select_ui_font(Platform::Linux, "zh-CN", &fonts), None);
    }

    #[gpui_pre::test]
    fn selected_family_survives_builtin_theme_changes(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            use gpui_kit::component::{Theme, ThemeMode};

            Theme::update(cx, |theme| theme.font_family = "Noto Sans CJK SC".into());
            Theme::change(ThemeMode::Dark, None, cx);
            assert_eq!(Theme::global(cx).font_family, "Noto Sans CJK SC");
            Theme::change(ThemeMode::Light, None, cx);
            assert_eq!(Theme::global(cx).font_family, "Noto Sans CJK SC");
        });
    }
}
