use rust_i18n::t;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    Light,
    Dark,
    System,
}

impl ThemePreference {
    pub fn label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::Light => t!("Light"),
            Self::Dark => t!("Dark"),
            Self::System => t!("System"),
        }
    }
}

/// 界面语言偏好。`System` 跟随操作系统；检测失败时回退英文。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LanguagePreference {
    #[default]
    System,
    English,
    ChineseSimplified,
    ChineseTraditional,
}

impl LanguagePreference {
    pub const ALL: [Self; 4] = [
        Self::System,
        Self::English,
        Self::ChineseSimplified,
        Self::ChineseTraditional,
    ];

    pub fn locale_code(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::English => "en",
            Self::ChineseSimplified => "zh-CN",
            Self::ChineseTraditional => "zh-TW",
        }
    }

    pub fn native_label(self) -> std::borrow::Cow<'static, str> {
        match self {
            Self::System => t!("System"),
            Self::English => std::borrow::Cow::Borrowed("English"),
            Self::ChineseSimplified => std::borrow::Cow::Borrowed("简体中文"),
            Self::ChineseTraditional => std::borrow::Cow::Borrowed("繁體中文"),
        }
    }

    pub fn resolve(self) -> &'static str {
        match self {
            Self::System => locale_from_os_tag(&sys_locale::get_locale().unwrap_or_default()),
            other => other.locale_code(),
        }
    }
}

/// 由操作系统语言标签推断本应用支持的 locale。
pub fn locale_from_os_tag(tag: &str) -> &'static str {
    let lower = tag.trim().to_lowercase();
    if !lower.starts_with("zh") {
        return "en";
    }
    let traditional = ["zh-tw", "zh-hk", "zh-mo", "zh-hant", "zh-tw_"]
        .iter()
        .any(|prefix| lower.starts_with(prefix) || lower.contains("hant"));
    if traditional {
        "zh-TW"
    } else {
        "zh-CN"
    }
}
