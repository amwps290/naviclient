# 界面视觉与版式重构实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把界面改成“明亮杂志”风格（暖白底、衬线大标题、悬浮卡片、玫瑰红品牌色、模块化首页货架），功能和交互不变。

**Architecture:** 新增 `src/app/style/`，集中定义颜色、字号、圆角、表面、网格和封面铺底，并覆盖组件库主题。然后按窗口框架、首页、媒体库、详情页、歌曲列表、正在播放、设置的顺序逐块改造。每个任务结束时程序都能运行、验证全部通过，由用户查看效果。

**Tech Stack:** Rust、GPUI（`gpui_kit` 0.7.1 / `gpui-pre` 0.3.8）、`rust-i18n`、`chrono`。

**Spec:** `docs/superpowers/specs/2026-11-06-ui-redesign-design.md`

## Global Constraints

- 直接在工作区修改，不创建分支，不提交，不执行 `git add`。提交由用户自己做。
- 所有 Cargo 命令最多 8 个并行任务（`-j 8`）。
- 不改变任何交互：歌曲行单击选择、双击播放，右键菜单、多选、快捷键、折叠、拖动保持原样；不删除任何按钮、菜单项、提示文字。
- 不改音频、播放、数据、配置格式；不打包字体文件；不新增依赖。
- 元素 ID 不变（GPUI 用它保留悬停和焦点状态），除非该元素被删除。
- 新增文案键先写在 `t!()` 里，再补到 `locales/en.yml`、`zh-CN.yml`、`zh-TW.yml`。
- Bash 工具运行在开了 `noclobber` 的 zsh 里：多步脚本写成文件后用 `bash` 运行。
- `src/audio` 下有两个已知的间歇失败测试：`cancelled_decoder_read_cannot_accumulate_speculative_workers` 和 `connection_failure_diagnostics_do_not_expose_signed_stream_urls`（及同时失败的并行测试）。出现时重跑一次；其他失败必须调查。

每个任务结束都运行这组**标准验证**，全部退出码为 0 才算通过：

```bash
cd /home/hn/code/rust/naviclient
cargo fmt -- --check
cargo test --locked -j 8 --all-targets
cargo clippy --locked -j 8 --all-targets -- -D warnings
cargo build --locked -j 8
```

每个任务最后一步都是请用户查看：运行 `cargo run --locked -j 8`（或让用户重启自己的调试实例），告诉用户本任务改了什么、看哪里，等用户反馈后再进入下一任务。这台机器上 KWin 拒绝截图请求，所以视觉结果以用户反馈为准。

## Review Focus

1. **很暗、很艳或灰色的封面铺底：** 文字仍然清楚（不低于 4.5:1），不会出现黑底压字或灰按钮。Task 4 的 `cover_tint` 遍历测试。
2. **封面调色板还没加载或拿不到：** 卡片和详情页不铺色、也不出错，加载完成后自然变色。Task 4 的回退测试。
3. **窗口很窄、全屏、开关队列抽屉、折叠侧栏：** 封面列数立刻变化且上下边缘对齐，不会停在旧宽度。Task 3、Task 4 的布局计算测试。
4. **运行中切换深浅色或跟随系统：** 设计规则和组件库颜色同时切换，不会一半新一半旧。Task 1 的主题切换测试。
5. **运行中切换界面语言：** 衬线字体跟着简繁体切换；目标语言没有衬线字体时回退无衬线，不会沿用上一个字体。Task 1 的字体配置测试。

---

### Task 1: 设计规则与全局主题

**Files:**
- Create: `src/app/style.rs`、`src/app/style/tokens.rs`、`src/app/style/tokens/tests.rs`、`src/app/style/theme.rs`、`src/app/style/text.rs`
- Modify: `src/app.rs`、`src/app/prelude.rs`、`src/main.rs`、`src/typography.rs`、`src/app/settings/preferences.rs`、`src/app/tests.rs`、`src/app/widgets/page_controls.rs`

**Interfaces:**
- Produces: `tokens::palette(cx: &App) -> Palette`，字段都是 `Hsla`：`page, raised, sunken, text, text_secondary, text_muted, brand, brand_hover, brand_active, brand_soft, on_brand, border, online`；`tokens::LIGHT`、`tokens::DARK`（`PaletteHex`）；`text::serif_heading(text, size: f32, cx: &App) -> Div`；`typography::serif_font_family(cx: &App) -> SharedString`、`typography::configure_serif_font(cx, locale)`；`app::apply_app_theme(cx)`。

- [ ] **步骤 1：写设计规则模块和失败的对比度测试**

`src/app/style.rs`：

```rust
//! Design rules shared by every page: tokens, theme overrides and text styles.

pub(in crate::app) mod text;
pub(in crate::app) mod theme;
// Some tokens gain their first reader in later tasks; Task 9 removes this.
#[allow(dead_code)]
pub(in crate::app) mod tokens;
```

`src/app/style/tokens.rs`：

```rust
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
    pub(in crate::app) on_brand: u32,
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
    brand_hover: 0xA32F56,
    brand_active: 0x8F294B,
    brand_soft: 0xF2E4E3,
    on_brand: 0xFFFFFF,
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
    on_brand: 0x1C1917,
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
    pub(in crate::app) brand_active: Hsla,
    pub(in crate::app) brand_soft: Hsla,
    pub(in crate::app) on_brand: Hsla,
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
            brand_active: color(self.brand_active),
            brand_soft: color(self.brand_soft),
            on_brand: color(self.on_brand),
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

#[cfg(test)]
mod tests;
```

`src/app/style/tokens/tests.rs`：

```rust
use super::*;
use crate::app::covers::artwork::contrast_ratio;

const AA: f32 = 4.5;

fn assert_readable(name: &str, foreground: Hsla, background: Hsla) {
    let ratio = contrast_ratio(foreground, background);
    assert!(ratio >= AA, "{name} is {ratio:.2}:1");
}

fn assert_palette(p: Palette) {
    for (surface_name, surface) in [
        ("page", p.page),
        ("raised", p.raised),
        ("sunken", p.sunken),
    ] {
        assert_readable(&format!("text on {surface_name}"), p.text, surface);
        assert_readable(
            &format!("secondary on {surface_name}"),
            p.text_secondary,
            surface,
        );
        assert_readable(&format!("muted on {surface_name}"), p.text_muted, surface);
        assert_readable(&format!("brand on {surface_name}"), p.brand, surface);
    }
    assert_readable("brand on soft", p.brand, p.brand_soft);
    assert_readable("text on soft", p.text, p.brand_soft);
    assert_readable("on-brand on brand", p.on_brand, p.brand);
    assert_readable("on-brand on hover", p.on_brand, p.brand_hover);
    assert_readable("on-brand on active", p.on_brand, p.brand_active);
}

#[test]
fn light_palette_meets_aa() {
    assert_palette(LIGHT.resolve());
}

#[test]
fn dark_palette_meets_aa() {
    assert_palette(DARK.resolve());
}
```

`src/app.rs` 的模块列表加入 `mod style;`（在 `mod sound;` 和 `mod widgets;` 之间）。

- [ ] **步骤 2：运行测试，确认能编译并通过**

运行：`cargo test --locked -j 8 style::tokens`

预期：2 个测试通过。这两个测试校验的是数值表，所以它们一开始就应该通过；如果失败，说明数值和 spec 不一致，应修数值。

- [ ] **步骤 3：写主题覆盖，并把 `src/app/tests.rs` 的测试改成新行为**

`src/app/style/theme.rs`：

```rust
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
    c.selection = hex(p.brand_soft);
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
```

在 `src/app.rs` 里删除 `adjust_light_theme_contrast` 整个函数及其文档注释，并在 `pub(crate) use` 列表前加：

```rust
pub(crate) use style::theme::apply_app_theme;
```

`src/main.rs` 把 `app::adjust_light_theme_contrast(cx);` 改成 `app::apply_app_theme(cx);`，并在 `typography::configure_ui_font(cx, ui_locale);` 后加 `typography::configure_serif_font(cx, ui_locale);`。

把 `src/app/tests.rs` 的整个文件替换成：

```rust
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
```

- [ ] **步骤 4：写衬线字体的失败测试**

在 `src/typography.rs` 的 `mod tests` 末尾加：

```rust
    #[test]
    fn linux_serif_prefers_the_matching_script() {
        let fonts = installed(&["Noto Serif CJK TC", "Noto Serif CJK SC"]);
        assert_eq!(
            select_serif_font(Platform::Linux, "zh-CN", &fonts).as_deref(),
            Some("Noto Serif CJK SC")
        );
        assert_eq!(
            select_serif_font(Platform::Linux, "zh-TW", &fonts).as_deref(),
            Some("Noto Serif CJK TC")
        );
    }

    #[test]
    fn macos_serif_uses_songti() {
        let fonts = installed(&["STSong", "Songti SC", "Songti TC"]);
        assert_eq!(
            select_serif_font(Platform::Macos, "zh-CN", &fonts).as_deref(),
            Some("Songti SC")
        );
        assert_eq!(
            select_serif_font(Platform::Macos, "zh-Hant", &fonts).as_deref(),
            Some("Songti TC")
        );
    }

    #[test]
    fn windows_has_no_serif_candidates() {
        let fonts = installed(&["Noto Serif CJK SC", "SimSun"]);
        assert_eq!(select_serif_font(Platform::Windows, "zh-CN", &fonts), None);
    }

    #[test]
    fn missing_serif_returns_none() {
        let fonts = installed(&["DejaVu Sans"]);
        assert_eq!(select_serif_font(Platform::Linux, "en", &fonts), None);
    }

    #[gpui_pre::test]
    fn serif_family_falls_back_to_the_ui_font(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            Theme::update(cx, |theme| theme.font_family = "Noto Sans CJK SC".into());
            assert_eq!(serif_font_family(cx), "Noto Sans CJK SC");

            apply_serif_font(cx, Some("Noto Serif CJK SC".into()));
            assert_eq!(serif_font_family(cx), "Noto Serif CJK SC");

            // A later language without a candidate must not keep the old family.
            apply_serif_font(cx, None);
            assert_eq!(serif_font_family(cx), "Noto Sans CJK SC");
        });
    }
```

运行：`cargo test --locked -j 8 typography`

预期：编译失败，提示 `select_serif_font` 未定义。

- [ ] **步骤 5：实现衬线字体选择**

在 `src/typography.rs` 里把 `use gpui_kit::{App, SharedString};` 改成 `use gpui_kit::{App, Global, SharedString};`，在 `prefers_traditional_chinese` 之前加候选表：

```rust
const LINUX_SC_SERIF: &[&str] = &[
    "Noto Serif CJK SC",
    "Noto Serif SC",
    "Source Han Serif SC",
    "Source Han Serif CN",
    "Noto Serif CJK TC",
];

const LINUX_TC_SERIF: &[&str] = &[
    "Noto Serif CJK TC",
    "Noto Serif TC",
    "Source Han Serif TC",
    "Source Han Serif TW",
    "Noto Serif CJK SC",
];

const MACOS_SC_SERIF: &[&str] = &["Songti SC", "STSong", "Songti TC"];

const MACOS_TC_SERIF: &[&str] = &["Songti TC", "Songti SC", "STSong"];
```

把 `select_ui_font` 替换成下面的代码，并在 `configure_ui_font` 之后加入其余函数：

```rust
fn first_installed(candidates: &[&str], installed: &[String]) -> Option<String> {
    candidates.iter().find_map(|candidate| {
        installed
            .iter()
            .find(|family| family.eq_ignore_ascii_case(candidate))
            .cloned()
    })
}

fn select_ui_font(platform: Platform, locale: &str, installed: &[String]) -> Option<String> {
    first_installed(candidates(platform, locale), installed)
}

fn serif_candidates(platform: Platform, locale: &str) -> &'static [&'static str] {
    match (platform, prefers_traditional_chinese(locale)) {
        // Windows ships no CJK serif worth using; headings stay bold sans.
        (Platform::Windows, _) => &[],
        (Platform::Macos, false) => MACOS_SC_SERIF,
        (Platform::Macos, true) => MACOS_TC_SERIF,
        (Platform::Linux | Platform::Other, false) => LINUX_SC_SERIF,
        (Platform::Linux | Platform::Other, true) => LINUX_TC_SERIF,
    }
}

fn select_serif_font(platform: Platform, locale: &str, installed: &[String]) -> Option<String> {
    first_installed(serif_candidates(platform, locale), installed)
}

struct SerifFont(Option<SharedString>);

impl Global for SerifFont {}

fn apply_serif_font(cx: &mut App, family: Option<SharedString>) {
    cx.set_global(SerifFont(family));
}

/// Select the heading family for this platform and language.
pub fn configure_serif_font(cx: &mut App, locale: &str) {
    let installed = cx.text_system().all_font_names();
    let family = select_serif_font(Platform::current(), locale, &installed);
    log::info!("selected serif font family={family:?}; locale={locale}");
    apply_serif_font(cx, family.map(SharedString::from));
}

/// Falls back to the UI family so headings always name an installed font.
pub fn serif_font_family(cx: &App) -> SharedString {
    cx.try_global::<SerifFont>()
        .and_then(|font| font.0.clone())
        .unwrap_or_else(|| Theme::global(cx).font_family.clone())
}
```

`src/app/settings/preferences.rs` 的 `set_language` 里，在 `crate::typography::configure_ui_font(cx, locale);` 后加：

```rust
        crate::typography::configure_serif_font(cx, locale);
```

运行：`cargo test --locked -j 8 typography`

预期：全部通过。

- [ ] **步骤 6：写文字样式并用在页面标题上**

`src/app/style/text.rs`：

```rust
//! Serif heading styles. Body text keeps the kit's UI font.

use gpui_kit::{div, px, relative, App, Div, FontWeight, ParentElement, SharedString, Styled};

use crate::typography::serif_font_family;

pub(in crate::app) fn serif_heading(text: impl Into<SharedString>, size: f32, cx: &App) -> Div {
    div()
        .font_family(serif_font_family(cx))
        .font_weight(FontWeight::BOLD)
        .text_size(px(size))
        .line_height(relative(1.2))
        .child(text.into())
}
```

`src/app/prelude.rs` 在 `pub(in crate::app) use crate::app::songs::scope::SongScope;` 之后加：

```rust
pub(in crate::app) use crate::app::style::text::serif_heading;
pub(in crate::app) use crate::app::style::tokens::{self, palette};
```

`src/app/widgets/page_controls.rs` 的 `page_header` 替换为：

```rust
    pub(in crate::app) fn page_header(
        &self,
        title: impl Into<SharedString>,
        subtitle: impl Into<SharedString>,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        v_flex()
            .gap_1()
            .child(serif_heading(title, tokens::PAGE_TITLE, cx))
            .child(
                div()
                    .text_size(px(tokens::META))
                    .text_color(palette(cx).text_secondary)
                    .child(subtitle.into()),
            )
            .into_any_element()
    }
```

- [ ] **步骤 7：运行标准验证**

预期：全部退出码为 0，测试数增加。

- [ ] **步骤 8：请用户查看**

告诉用户：全局底色已是暖白，组件库按钮和滑块已是玫瑰红，媒体库页面标题是衬线体。版式还没变。请用户确认标题字体和颜色观感，并切换一次深色、一次界面语言。

---

### Task 2: 表面、按钮与封面图样式

**Files:**
- Create: `src/app/style/surface.rs`、`src/app/style/buttons.rs`
- Modify: `src/app/style.rs`、`src/app/style/text.rs`、`src/app/style/tokens.rs`、`src/app/prelude.rs`、`src/app/covers/view.rs`、`src/app/widgets/favorite_button.rs`、`src/app/player/controls.rs`、`src/app/recommendations/daily_mix_view.rs`

**Interfaces:**
- Consumes: `tokens::palette`。
- Produces: `surface::raised_surface<E: Styled>(el: E, radius: f32, cx: &App) -> E`；`surface::cover_shadow(hovered: bool, cx: &App) -> Vec<BoxShadow>`；`buttons::brand_button_style(cx: &App) -> ButtonCustomVariant`、`buttons::capsule_button_style(cx: &App)`、`buttons::soft_toggle_style(active: bool, cx: &App)`、`buttons::round_play_button(id, icon, label, size: f32, cx: &App) -> Button`、`buttons::capsule_button(id, cx: &App) -> Button`；`text::section_title(text, cx: &App) -> Div`；`NavidromeApp::render_cover_styled(&self, cover_id: Option<&str>, size: f32, radius: f32, shadows: Vec<BoxShadow>) -> AnyElement`；`tokens::radius::{CONTROL, ROW, COVER, LARGE, FULL}`、`tokens::SECTION_TITLE`、`tokens::BODY`、`tokens::CAPTION`。

这个任务放后面都要用的基础件，并立刻接到现有的播放按钮、开关和收藏按钮上。其中 `raised_surface`、`section_title`、`capsule_button`、`round_play_button`、`cover_shadow` 要到 Task 3、4 才有调用方，所以这几个模块暂时带 `#[allow(dead_code)]`，Task 9 统一移除。

- [ ] **步骤 1：加入常量**

在 `src/app/style/tokens.rs` 的 `// Type scale.` 段落里补上，并加入圆角：

```rust
pub(in crate::app) const SECTION_TITLE: f32 = 18.0;
pub(in crate::app) const BODY: f32 = 14.0;
pub(in crate::app) const CAPTION: f32 = 12.0;

pub(in crate::app) mod radius {
    pub(in crate::app) const CONTROL: f32 = 8.0;
    pub(in crate::app) const ROW: f32 = 10.0;
    pub(in crate::app) const COVER: f32 = 12.0;
    pub(in crate::app) const LARGE: f32 = 20.0;
    pub(in crate::app) const FULL: f32 = 999.0;
}
```

- [ ] **步骤 2：写表面样式**

`src/app/style/surface.rs`：

```rust
//! Raised surfaces and cover shadows. Shadows only go on things that float.

use gpui_kit::component::{box_shadow, ActiveTheme};
use gpui_kit::{px, App, BoxShadow, Hsla, Styled};

use super::tokens::palette;

fn shadow_color(cx: &App, opacity: f32) -> Hsla {
    palette(cx).text.opacity(opacity)
}

/// Dark mode has no visible shadow, so raised things also get a lighter
/// surface and a 1px border.
pub(in crate::app) fn raised_surface<E: Styled>(element: E, radius: f32, cx: &App) -> E {
    let p = palette(cx);
    let element = element.bg(p.raised).rounded(px(radius));
    if cx.theme().is_dark() {
        element.border_1().border_color(p.border).shadow(vec![box_shadow(
            px(0.0),
            px(8.0),
            px(24.0),
            px(0.0),
            gpui_kit::hsla(0.0, 0.0, 0.0, 0.35),
        )])
    } else {
        element.shadow(vec![
            box_shadow(px(0.0), px(1.0), px(2.0), px(0.0), shadow_color(cx, 0.06)),
            box_shadow(px(0.0), px(8.0), px(24.0), px(0.0), shadow_color(cx, 0.10)),
        ])
    }
}

pub(in crate::app) fn cover_shadow(hovered: bool, cx: &App) -> Vec<BoxShadow> {
    let dark = cx.theme().is_dark();
    let color = if dark {
        gpui_kit::hsla(0.0, 0.0, 0.0, if hovered { 0.6 } else { 0.4 })
    } else {
        shadow_color(cx, if hovered { 0.24 } else { 0.14 })
    };
    let (y, blur) = if hovered { (10.0, 22.0) } else { (6.0, 16.0) };
    vec![box_shadow(px(0.0), px(y), px(blur), px(0.0), color)]
}
```

- [ ] **步骤 3：写按钮样式**

`src/app/style/buttons.rs`：

```rust
//! Brand, capsule and toggle button recipes shared by the whole app.

use gpui_kit::component::button::{Button, ButtonCustomVariant, ButtonVariants};
use gpui_kit::{px, App, ElementId, SharedString, Styled};

use super::tokens::{palette, radius};
use crate::assets::PlayerIcon;

pub(in crate::app) fn brand_button_style(cx: &App) -> ButtonCustomVariant {
    let p = palette(cx);
    ButtonCustomVariant::new(cx)
        .color(p.brand)
        .foreground(p.on_brand)
        .hover(p.brand_hover)
        .active(p.brand_active)
}

pub(in crate::app) fn capsule_button_style(cx: &App) -> ButtonCustomVariant {
    let p = palette(cx);
    ButtonCustomVariant::new(cx)
        .color(p.sunken)
        .foreground(p.text)
        .hover(p.border)
        .active(p.border)
}

/// Quiet while off; brand color only while the mode is on.
pub(in crate::app) fn soft_toggle_style(active: bool, cx: &App) -> ButtonCustomVariant {
    let p = palette(cx);
    if active {
        ButtonCustomVariant::new(cx)
            .color(p.brand_soft)
            .foreground(p.brand)
            .hover(p.brand_soft)
            .active(p.brand_soft)
    } else {
        ButtonCustomVariant::new(cx)
            .color(p.page.opacity(0.0))
            .foreground(p.text_secondary)
            .hover(p.sunken)
            .active(p.border)
    }
}

pub(in crate::app) fn round_play_button(
    id: impl Into<ElementId>,
    icon: PlayerIcon,
    label: impl Into<SharedString>,
    size: f32,
    cx: &App,
) -> Button {
    let label = label.into();
    Button::new(id)
        .icon(icon)
        .accessibility_label(label.clone())
        .tooltip(label)
        .custom(brand_button_style(cx))
        .rounded(px(radius::FULL))
        .w(px(size))
        .h(px(size))
}

pub(in crate::app) fn capsule_button(id: impl Into<ElementId>, cx: &App) -> Button {
    Button::new(id)
        .custom(capsule_button_style(cx))
        .rounded(px(radius::FULL))
}
```

把 `src/app/style.rs` 的模块声明替换为：

```rust
// Some items gain their first caller in later tasks; Task 9 removes these.
#[allow(dead_code)]
pub(in crate::app) mod buttons;
#[allow(dead_code)]
pub(in crate::app) mod surface;
#[allow(dead_code)]
pub(in crate::app) mod text;
pub(in crate::app) mod theme;
#[allow(dead_code)]
pub(in crate::app) mod tokens;
```

`src/app/style/text.rs` 末尾加：

```rust
pub(in crate::app) fn section_title(text: impl Into<SharedString>, cx: &App) -> Div {
    serif_heading(text, super::tokens::SECTION_TITLE, cx)
}
```

`src/app/prelude.rs` 把 Task 1 加的两行替换为：

```rust
pub(in crate::app) use crate::app::style::buttons::{brand_button_style, soft_toggle_style};
pub(in crate::app) use crate::app::style::text::serif_heading;
pub(in crate::app) use crate::app::style::tokens::{self, palette};
```

其余名称在第一次使用它们的任务里再加进 prelude（未使用的重导出会被 clippy 拒绝）。

- [ ] **步骤 4：把封面样式接到统一入口**

`src/app/covers/view.rs` 的 `render_cover` 替换为：

```rust
    pub(in crate::app) fn render_cover(
        &self,
        cover_id: Option<&str>,
        size: f32,
        _cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        self.render_cover_styled(cover_id, size, tokens::radius::COVER, Vec::new())
    }

    pub(in crate::app) fn render_cover_styled(
        &self,
        cover_id: Option<&str>,
        size: f32,
        radius: f32,
        shadows: Vec<gpui_kit::BoxShadow>,
    ) -> gpui_kit::AnyElement {
        let cover = cover_id
            .and_then(|id| self.state.covers.image(id))
            .cloned()
            .unwrap_or_else(|| self.default_cover.clone());
        self.mark_cover_image_used(&cover);
        img(cover)
            .size(px(size))
            // Override GPUI's intrinsic image ratio so portrait artwork cannot grow the slot.
            .aspect_square()
            .object_fit(ObjectFit::Cover)
            .flex_none()
            .rounded(px(radius))
            .shadow(shadows)
            .into_any_element()
    }
```

- [ ] **步骤 5：播放开关和收藏按钮换成品牌色**

`src/app/player/controls.rs` 的 `player_toggle_style` 函数体替换为：

```rust
        soft_toggle_style(active, cx)
```

`src/app/widgets/favorite_button.rs` 把 `cx.theme().red` 改成 `palette(cx).brand`，把 `cx.theme().muted_foreground` 改成 `palette(cx).text_secondary`。

`render_player_transport` 里的 `.custom(play_button_style(accent, cx))` 改成 `.custom(brand_button_style(cx))`。`play_button_style` 还有迷你播放器在用，留到 Task 8 删除。`accent` 参数此时可能未使用，保留它的调用方传参到 Task 3，所以这里把它改名为 `_accent`。

`src/app/recommendations/daily_mix_view.rs` 暂时不动（Task 4 重写）。

- [ ] **步骤 6：运行标准验证**

预期：全部通过。

- [ ] **步骤 7：请用户查看**

告诉用户：播放按钮、随机和循环开关、收藏心形已是玫瑰红，不再随封面变色。

---

### Task 3: 窗口框架

**Files:**
- Create: `src/app/shell/layout.rs`、`src/app/shell/layout/tests.rs`
- Modify: `src/app/shell.rs`、`src/app/style/tokens.rs`、`src/app/shell/render.rs`、`src/app/shell/sidebar.rs`、`src/app/shell/titlebar.rs`、`src/app/player/bar.rs`、`src/app/player/controls.rs`、`src/app/queue/view.rs`、`src/app/sound/volume_view.rs`、`locales/en.yml`、`locales/zh-CN.yml`、`locales/zh-TW.yml`

**Interfaces:**
- Consumes: Task 2 的 `raised_surface`、`section_title`、`brand_button_style`、`soft_toggle_style`。
- Produces: `layout::ShellLayout { compact: bool, sidebar_collapsed: bool, auto_collapsed: bool, sidebar_width: f32, queue_drawer_width: f32, player_margin: f32, page_padding: f32, content_width: f32 }`；`NavidromeApp::shell_layout(&self, window: &Window) -> ShellLayout`。

- [ ] **步骤 1：写布局计算的失败测试**

`src/app/shell/layout/tests.rs`：

```rust
use super::*;

fn input(width: f32) -> ShellInput {
    ShellInput {
        viewport_width: width,
        window_padding: 0.0,
        sidebar_preference_collapsed: false,
        queue_open: false,
    }
}

#[test]
fn regular_window_uses_full_sidebar_and_page_padding() {
    let layout = shell_layout(input(1280.0));
    assert!(!layout.compact);
    assert!(!layout.sidebar_collapsed);
    assert_eq!(layout.sidebar_width, 200.0);
    assert_eq!(layout.page_padding, 32.0);
    assert_eq!(layout.player_margin, 12.0);
    assert_eq!(layout.content_width, 1280.0 - 200.0 - 64.0);
}

#[test]
fn compact_window_auto_collapses_the_sidebar() {
    let layout = shell_layout(input(1039.0));
    assert!(layout.compact);
    assert!(layout.auto_collapsed);
    assert_eq!(layout.sidebar_width, 56.0);
    assert_eq!(layout.page_padding, 20.0);
    assert_eq!(layout.player_margin, 8.0);
    assert_eq!(layout.queue_drawer_width, 320.0);
    assert_eq!(layout.content_width, 1039.0 - 56.0 - 40.0);
}

#[test]
fn threshold_width_is_regular() {
    assert!(!shell_layout(input(1040.0)).compact);
}

#[test]
fn collapsed_preference_is_not_auto_collapse() {
    let layout = shell_layout(ShellInput {
        sidebar_preference_collapsed: true,
        ..input(1280.0)
    });
    assert!(layout.sidebar_collapsed);
    assert!(!layout.auto_collapsed);
    assert_eq!(layout.content_width, 1280.0 - 56.0 - 64.0);
}

#[test]
fn open_queue_drawer_takes_width_and_margin_from_content() {
    let layout = shell_layout(ShellInput {
        queue_open: true,
        ..input(1280.0)
    });
    assert_eq!(layout.content_width, 1280.0 - 200.0 - 380.0 - 12.0 - 64.0);
}

#[test]
fn window_shadow_padding_is_excluded() {
    let layout = shell_layout(ShellInput {
        window_padding: 24.0,
        ..input(1280.0)
    });
    assert_eq!(layout.content_width, 1280.0 - 24.0 - 200.0 - 64.0);
}

#[test]
fn absurdly_small_window_never_goes_negative() {
    assert_eq!(shell_layout(input(10.0)).content_width, 0.0);
}
```

运行：`cargo test --locked -j 8 shell::layout`

预期：编译失败，`shell_layout` 未定义（先做步骤 2 的模块声明才会走到这里）。

- [ ] **步骤 2：实现布局计算**

`src/app/style/tokens.rs` 末尾（`#[cfg(test)]` 之前）加：

```rust
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
pub(in crate::app) const PAGE_PADDING: f32 = 32.0;
pub(in crate::app) const PAGE_PADDING_COMPACT: f32 = 20.0;
```

`src/app/shell.rs` 加 `pub(in crate::app) mod layout;`。

`src/app/shell/layout.rs`：

```rust
//! Window-level layout arithmetic. Computed from the current frame's window
//! size so grids never lag behind a resize or a drawer toggle.

use super::view_helpers::compact_window_layout;
use crate::app::prelude::*;

#[derive(Clone, Copy, Debug)]
struct ShellInput {
    viewport_width: f32,
    window_padding: f32,
    sidebar_preference_collapsed: bool,
    queue_open: bool,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::app) struct ShellLayout {
    pub(in crate::app) compact: bool,
    pub(in crate::app) sidebar_collapsed: bool,
    pub(in crate::app) auto_collapsed: bool,
    pub(in crate::app) sidebar_width: f32,
    pub(in crate::app) queue_drawer_width: f32,
    pub(in crate::app) player_margin: f32,
    pub(in crate::app) page_padding: f32,
    pub(in crate::app) content_width: f32,
}

fn shell_layout(input: ShellInput) -> ShellLayout {
    let compact = compact_window_layout(input.viewport_width);
    let sidebar_collapsed = input.sidebar_preference_collapsed || compact;
    let sidebar_width = if sidebar_collapsed {
        tokens::SIDEBAR_COLLAPSED_WIDTH
    } else {
        tokens::SIDEBAR_WIDTH
    };
    let queue_drawer_width = if compact {
        tokens::QUEUE_DRAWER_COMPACT_WIDTH
    } else {
        tokens::QUEUE_DRAWER_WIDTH
    };
    let page_padding = if compact {
        tokens::PAGE_PADDING_COMPACT
    } else {
        tokens::PAGE_PADDING
    };
    let drawer = if input.queue_open {
        queue_drawer_width + tokens::QUEUE_DRAWER_MARGIN
    } else {
        0.0
    };
    let content_width = (input.viewport_width
        - input.window_padding
        - sidebar_width
        - drawer
        - page_padding * 2.0)
        .max(0.0);
    ShellLayout {
        compact,
        sidebar_collapsed,
        auto_collapsed: compact,
        sidebar_width,
        queue_drawer_width,
        player_margin: if compact {
            tokens::PLAYER_COMPACT_MARGIN
        } else {
            tokens::PLAYER_MARGIN
        },
        page_padding,
        content_width,
    }
}

impl NavidromeApp {
    pub(in crate::app) fn shell_layout(&self, window: &Window) -> ShellLayout {
        let paddings = window_paddings(window);
        shell_layout(ShellInput {
            viewport_width: f32::from(window.viewport_size().width),
            window_padding: f32::from(paddings.left + paddings.right),
            sidebar_preference_collapsed: self.config.sidebar_collapsed,
            queue_open: self.state.queue_drawer_open,
        })
    }
}

#[cfg(test)]
mod tests;
```

运行：`cargo test --locked -j 8 shell::layout`

预期：7 个测试通过。

- [ ] **步骤 3：改主窗口顶层布局**

`src/app/prelude.rs` 在 `style::buttons` 那一行之后加：

```rust
pub(in crate::app) use crate::app::style::surface::raised_surface;
```

并把 `style::text` 那一行改成 `pub(in crate::app) use crate::app::style::text::{section_title, serif_heading};`。

`src/app/shell/render.rs` 里，把 `let settings_open = ...` 到 `let queue_drawer_width = ...;` 这几行替换为：

```rust
        let settings_open = self.state.settings.open;
        let layout = self.shell_layout(window);
```

把 NowPlaying 分支里的 `queue_drawer_width` 改成 `layout.queue_drawer_width`，`auto_collapsed_sidebar` 改成 `layout.auto_collapsed`。

把函数末尾主窗口的 `v_flex()`（第 119 行起）整段替换为：

```rust
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(
                TitleBar::new()
                    .on_close_window(Self::close_button_handler(cx))
                    .child(self.render_main_titlebar(cx)),
            )
            .child(
                h_flex()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(self.render_sidebar(&layout, cx))
                    .child(
                        div()
                            .id("content")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.content_scroll_handle)
                            .on_scroll_wheel(cx.listener(|this, _, _, _| {
                                this.maybe_load_more_content();
                            }))
                            .p(px(layout.page_padding))
                            .child(self.render_content(window, cx)),
                    )
                    .when(self.state.queue_drawer_open, |row| {
                        // 普通浏览页使用真正的分栏，让专辑网格和歌曲列表
                        // 获得抽屉打开后的实际宽度并自然重排，而不是被覆盖裁切。
                        row.child(self.render_queue_drawer(layout.queue_drawer_width, false, cx))
                    })
                    // 设置为内容区覆盖层：点击背景关闭，标题栏与播放条保持可操作。
                    .when(settings_open, |row| {
                        row.child(self.render_settings_overlay(layout.auto_collapsed, cx))
                    }),
            )
            .child(self.render_player(window, cx))
            .into_any_element()
```

删除文件顶部的 `use super::view_helpers::compact_window_layout;`。

- [ ] **步骤 4：改侧栏**

`src/app/shell/sidebar.rs` 把 `impl NavidromeApp { ... }` 整段替换为下面的代码；同时删除文件顶部的 `use crate::app::covers::artwork::{...};`，并加上 `use super::layout::ShellLayout;`。`impl View` 部分保留。

```rust
impl NavidromeApp {
    pub(in crate::app) fn render_sidebar(
        &self,
        layout: &ShellLayout,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let collapsed = layout.sidebar_collapsed;
        v_flex()
            .w(px(layout.sidebar_width))
            .h_full()
            .flex_none()
            .gap_1()
            .when(collapsed, |this| this.p_2())
            .when(!collapsed, |this| this.p_3())
            .child(self.nav_button(View::Home, None, collapsed, cx))
            .child(self.nav_button(
                View::Favorites,
                Some(self.state.favorites.total()),
                collapsed,
                cx,
            ))
            .when(!collapsed, |this| {
                this.child(
                    div()
                        .px_3()
                        .pt_4()
                        .pb_2()
                        .text_size(px(tokens::CAPTION))
                        .text_color(palette(cx).text_muted)
                        .child(t!("Library")),
                )
            })
            .when(collapsed, |this| this.child(div().h_2().flex_none()))
            .child(self.nav_button(
                View::Artists,
                Some(self.state.artists.len()),
                collapsed,
                cx,
            ))
            .child(
                self.nav_button(
                    View::Albums,
                    // 专辑列表是增量分页加载，只有拉尽后总数才可信；
                    // 拉尽前不显示，避免给出误导性的“当前页数”。
                    self.state
                        .albums
                        .exhausted()
                        .then_some(self.state.albums.len()),
                    collapsed,
                    cx,
                ),
            )
            .child(self.nav_button(
                View::Genres,
                Some(self.state.genres.len()),
                collapsed,
                cx,
            ))
            .child(self.nav_button(
                View::Playlists,
                Some(self.state.playlists.items().len()),
                collapsed,
                cx,
            ))
            .child(self.nav_button(View::Search, None, collapsed, cx))
            .child(self.render_sidebar_footer(collapsed, layout.auto_collapsed, cx))
            .into_any_element()
    }

    /// 侧边栏的折叠/展开按钮；状态持久化到配置。
    fn sidebar_collapse_toggle(&self, collapsed: bool, cx: &Context<Self>) -> Button {
        Button::new("sidebar-collapse-toggle")
            .icon(if collapsed {
                KitIcon::PanelLeftOpen
            } else {
                KitIcon::PanelLeftClose
            })
            .tooltip(if collapsed {
                t!("Expand sidebar")
            } else {
                t!("Collapse sidebar")
            })
            .ghost()
            .small()
            .on_click(cx.listener(|this, _, _, cx| {
                this.config.sidebar_collapsed = !this.config.sidebar_collapsed;
                if let Err(error) = config::save(&this.config) {
                    this.notify_error(t!(
                        "Settings save failed: %{error}",
                        error = format!("{error:#}")
                    ));
                }
                cx.notify();
            }))
    }

    pub(in crate::app) fn nav_button(
        &self,
        view: View,
        count: Option<usize>,
        collapsed: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let (label, icon) = view.navigation_presentation();
        let nav_id = view.nav_id();
        let selected = view.is_selected_for(self.state.view);
        let p = palette(cx);
        if collapsed {
            // 折叠态：图标按钮居中、拉高加大点击区，标签退化为 tooltip。
            let button = Button::new(SharedString::from(format!("nav-{nav_id}")))
                .icon(icon)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.navigate_to(view);
                    cx.notify();
                }));
            div()
                .w_full()
                .flex()
                .justify_center()
                .child(
                    button
                        .tooltip(label)
                        .custom(soft_toggle_style(selected, cx))
                        .rounded(px(tokens::radius::CONTROL))
                        .w_full()
                        .h(px(34.0)),
                )
                .into_any_element()
        } else {
            h_flex()
                .id(SharedString::from(format!("nav-{nav_id}")))
                .w_full()
                .h(px(34.0))
                .items_center()
                .gap_2()
                .px_3()
                .rounded(px(tokens::radius::CONTROL))
                .text_color(if selected { p.brand } else { p.text_secondary })
                .when(selected, |this| this.bg(p.brand_soft))
                .when(!selected, |this| {
                    this.cursor_pointer().hover(|style| style.bg(p.sunken))
                })
                .child(icon.with_size(px(16.0)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(tokens::BODY))
                        .truncate()
                        .when(selected, |this| this.font_weight(FontWeight::SEMIBOLD))
                        .child(label),
                )
                .when_some(count.filter(|count| *count > 0), |this, count| {
                    this.child(
                        div()
                            .flex_none()
                            .text_size(px(tokens::CAPTION))
                            .text_color(p.text_muted)
                            .child(count.to_string()),
                    )
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.navigate_to(view);
                    cx.notify();
                }))
                .into_any_element()
        }
    }

    /// 侧边栏底部：当前服务器与折叠按钮。多服务器下必须一眼可见正在
    /// 浏览谁的库，点击进入设置切换。
    fn render_sidebar_footer(
        &self,
        collapsed: bool,
        auto_collapsed: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let connected = self.state.connection.server().is_some();
        let label = self.active_server_label();
        let status = if self.state.connection.loading() {
            t!("Connecting…")
        } else if connected {
            t!("Connected")
        } else {
            t!("Offline")
        };
        let server_count = self.config.servers.len();
        let tooltip = if server_count > 1 {
            t!(
                "%{label} — %{status}\n%{server_count} servers configured · click to switch",
                label = label,
                status = status,
                server_count = server_count
            )
            .to_string()
        } else {
            t!(
                "%{label} — %{status}\nClick to manage servers",
                label = label,
                status = status
            )
            .to_string()
        };

        if collapsed {
            return v_flex()
                .mt_auto()
                .w_full()
                .items_center()
                .gap_1()
                .child(
                    Button::new("sidebar-server")
                        .icon(if connected {
                            AppIcon::Server
                        } else {
                            AppIcon::ServerOff
                        })
                        .tooltip(tooltip)
                        .custom(soft_toggle_style(self.state.settings.open, cx))
                        .rounded(px(tokens::radius::CONTROL))
                        .w_full()
                        .h(px(34.0))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.state.settings.open = true;
                            this.state.settings.section = SettingsSection::Servers;
                            this.refresh_output_devices(cx);
                            cx.notify();
                        })),
                )
                .when(!auto_collapsed, |this| {
                    this.child(self.sidebar_collapse_toggle(true, cx))
                })
                .into_any_element();
        }

        h_flex()
            .mt_auto()
            .w_full()
            .items_center()
            .gap_1()
            .child(
                h_flex()
                    .id("sidebar-server")
                    .flex_1()
                    .min_w_0()
                    .h(px(32.0))
                    .items_center()
                    .gap_2()
                    .px_2()
                    .rounded(px(tokens::radius::CONTROL))
                    .cursor_pointer()
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                    .hover(|style| style.bg(p.sunken))
                    .child(
                        div()
                            .flex_none()
                            .size(px(7.0))
                            .rounded_full()
                            .bg(if connected { p.online } else { p.text_muted }),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .text_size(px(tokens::CAPTION))
                            .text_color(p.text_secondary)
                            .truncate()
                            .child(label),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.settings.open = true;
                        this.state.settings.section = SettingsSection::Servers;
                        this.refresh_output_devices(cx);
                        cx.notify();
                    })),
            )
            .child(self.sidebar_collapse_toggle(false, cx))
            .into_any_element()
    }
}
```

文案：在三个语言文件里把 `"LIBRARY"` 那一行替换为（en / zh-CN / zh-TW 依次）：

```yaml
"Library": "Library"
"Library": "媒体库"
"Library": "媒體庫"
```

- [ ] **步骤 5：清空主标题栏的页面名称**

`src/app/shell/titlebar.rs` 的 `render_main_titlebar` 替换为（页面名称改由内容区标题承担）：

```rust
    pub(in crate::app) fn render_main_titlebar(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let detail_parent = self.state.view.detail_parent();

        h_flex()
            .h_full()
            .w_full()
            .items_center()
            .justify_between()
            .pr_2()
            .child(div().min_w_0().when_some(detail_parent, |this, parent| {
                this.child(
                    div().occlude().child(
                        Button::new("titlebar-back")
                            .icon(AppIcon::ArrowLeft)
                            .tooltip(t!("Back to library"))
                            .ghost()
                            .small()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.state.navigate_to(parent);
                                cx.notify();
                            })),
                    ),
                )
            }))
            .child(self.render_titlebar_actions("toggle-mini-player", "title-settings", cx))
            .into_any_element()
    }
```

- [ ] **步骤 6：改播放条为悬浮卡片**

`src/app/player/bar.rs` 的 `render_player` 替换为：

```rust
    pub(in crate::app) fn render_player(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let playback = self.playback_for_display();
        let brand = palette(cx).brand;
        let viewport_width = f32::from(window.viewport_size().width);
        let layout = self.shell_layout(window);
        let compact_player = layout.compact;
        // Keep the technical chip on wider windows; details are also available
        // on the now-playing page.
        let show_technical_chip = viewport_width >= 1080.0;
        raised_surface(
            h_flex()
                .flex_none()
                .h(px(if compact_player {
                    tokens::PLAYER_COMPACT_HEIGHT
                } else {
                    tokens::PLAYER_HEIGHT
                }))
                .mx(px(layout.player_margin))
                .mb(px(layout.player_margin))
                .when(compact_player, |this| this.px_2())
                .when(!compact_player, |this| this.px_3())
                .gap(if compact_player { px(8.0) } else { px(16.0) })
                .items_center()
                .justify_between(),
            tokens::radius::LARGE,
            cx,
        )
        .child(self.render_player_track(compact_player, cx))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .items_center()
                .child(self.render_player_transport(&playback, compact_player, cx))
                .child(self.render_player_progress(&playback, brand, cx)),
        )
        .child(self.render_player_actions(compact_player, show_technical_chip, brand, cx))
        .into_any_element()
    }
```

在 `render_player_track` 里：
- `player_cover_size` 改为 `if compact_player { 40.0 } else { 48.0 }`。
- 两处 `.when(compact_player, |this| this.text_sm()).when(!compact_player, |this| this.text_base())` 都替换为 `.text_size(px(tokens::BODY))`。
- `.rounded(px(12.0))` 改为 `.rounded(px(tokens::radius::COVER))`。
- `.hover(|style| style.bg(cx.theme().accent.opacity(0.08)))` 改为 `.hover(|style| style.bg(palette(cx).sunken))`。

在 `render_player_actions` 里：
- 删除 `.border_l_1()` 和紧随其后的 `.border_color(cx.theme().border.opacity(0.8))`。
- 删除技术信息标签后面、音量按钮前面的两个分隔线 `div().h(px(24.0)).border_l_1()...`（只删这两个 `.child(...)` 块）。

`technical_info_chip` 替换为：

```rust
fn technical_info_chip(
    id: &'static str,
    label: String,
    cx: &Context<NavidromeApp>,
) -> gpui_kit::AnyElement {
    let p = palette(cx);
    div()
        .id(id)
        .h(px(20.0))
        .max_w(px(64.0))
        .px_1p5()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(tokens::radius::CONTROL))
        .bg(p.sunken)
        .text_size(px(tokens::CAPTION))
        .text_color(p.text_secondary)
        .truncate()
        .child(label)
        .into_any_element()
}
```

调用处改为 `technical_info_chip("player-format", info.format, cx)`。

`src/app/player/controls.rs`：把 `render_player_transport` 的签名改成 `(&self, playback: &PlaybackState, compact: bool, cx: &Context<Self>)`，并把播放按钮的 `.with_size(px(40.0))` 改成：

```rust
                    .with_size(px(if compact {
                        tokens::PLAY_BUTTON_COMPACT
                    } else {
                        tokens::PLAY_BUTTON
                    }))
```

`src/app/sound/volume_view.rs`：面板的 `.bottom(px(64.0))` 改为 `.bottom(px(56.0))`（播放条矮了 16，保持面板和卡片之间 5 像素间隙），`.rounded(px(6.0))` 改为 `.rounded(px(tokens::radius::COVER))`。

`render_stall_chip`（`src/app/player/progress.rs`）的 `.rounded(px(4.0))` 改为 `.rounded(px(tokens::radius::CONTROL))`。

- [ ] **步骤 7：改队列抽屉为悬浮卡片**

`src/app/queue/view.rs`：删除顶部的 `use crate::app::covers::artwork::{...};`。

`render_queue_drawer` 里 `v_flex()` 到 `.border_color(cx.theme().border)` 这段替换为：

```rust
        raised_surface(
            v_flex()
                .id("queue-drawer")
                .w(px(width))
                .min_w_0()
                .flex_none()
                .mb(px(tokens::QUEUE_DRAWER_MARGIN))
                .when(overlay, |this| {
                    this.absolute()
                        .top_0()
                        .right(px(tokens::QUEUE_DRAWER_MARGIN))
                        .bottom_0()
                })
                .when(!overlay, |this| this.mr(px(tokens::QUEUE_DRAWER_MARGIN)))
                .occlude()
                .overflow_hidden(),
            tokens::radius::LARGE,
            cx,
        )
```

`render_queue_drawer_header`：删除 `.border_b_1()` 和 `.border_color(cx.theme().border)`，把标题那个 `div().text_base().font_weight(FontWeight::SEMIBOLD).child(tab.label())` 替换为 `section_title(tab.label(), cx)`。

`render_queue_drawer_tabs`、`render_auto_continue_control`：删除 `.border_b_1()` 和 `.border_color(cx.theme().border.opacity(0.7))`。

`render_queue_action_bar`：删除开头的 `let accent = ...` 和 `let play_all_style = ...`，删除 `.bg(cx.theme().secondary.opacity(0.18))`、`.border_b_1()`、`.border_color(cx.theme().border.opacity(0.7))`，把 `.custom(play_all_style)` 改成 `.custom(brand_button_style(cx))`。

- [ ] **步骤 8：运行标准验证**

预期：全部通过。如果 clippy 报 `now_playing_accent`、`accent_foreground` 等未被使用，不要删 `now_playing_accent`（Task 7、8 还在用）；只删确实没有调用方的。

- [ ] **步骤 9：请用户查看**

告诉用户：看窗口框架。检查没有分割线、标题栏只剩按钮、侧栏分组和底部服务器行、悬浮播放条（列表最后一行不被挡住）、打开队列（按 Q）的悬浮抽屉，并拖窄窗口到 1040 以下、全屏各看一次。

---

### Task 4: 首页货架

**Files:**
- Create: `src/app/style/grid.rs`、`src/app/style/grid/tests.rs`、`src/app/style/cover_tint.rs`、`src/app/style/cover_tint/tests.rs`、`src/app/widgets/shelf.rs`、`src/app/home/greeting.rs`、`src/app/home/greeting/tests.rs`
- Modify: `src/app/style.rs`、`src/app/style/tokens.rs`、`src/app/prelude.rs`、`src/app/widgets.rs`、`src/app/home.rs`、`src/app/home/view.rs`、`src/app/home/feature_view.rs`、`src/app/home/section_view.rs`、`src/app/recommendations/daily_mix_view.rs`、`src/app/library/album_grid.rs`、`src/app/widgets/skeleton.rs`、`locales/*.yml`

**Interfaces:**
- Consumes: `ShellLayout::content_width`、`render_cover_styled`、`cover_shadow`、`round_play_button`、`capsule_button`、`section_title`、`serif_heading`。
- Produces: `grid::grid_columns(available: f32) -> GridColumns { count: usize, width: f32 }`；`NavidromeApp::content_columns(&self, window: &Window) -> GridColumns`；`cover_tint::{TintSurface, cover_tint, tint_background}`；`NavidromeApp::tint_for_cover(&self, surface: TintSurface, cover_id: Option<&str>, cx: &Context<Self>) -> Option<(Hsla, Hsla)>`；`shelf::{cover_grid, hero_shelf, shelf_section}`；`render_album_grid`、`render_album_skeleton_grid(window, cx)`（新增 `window` 参数）。

- [ ] **步骤 1：写列网格的失败测试**

`src/app/style/grid/tests.rs`：

```rust
use super::*;

#[test]
fn matches_the_spec_reference_widths() {
    let narrow = grid_columns(800.0);
    assert_eq!(narrow.count, 4);
    assert!((narrow.width - 185.0).abs() < 0.01);

    let regular = grid_columns(1016.0);
    assert_eq!(regular.count, 5);
    assert!((regular.width - 187.2).abs() < 0.01);
}

#[test]
fn columns_always_fill_the_available_width_exactly() {
    for available in [400.0, 777.0, 1016.0, 1600.0, 3000.0, 7000.0] {
        let grid = grid_columns(available);
        let used = grid.width * grid.count as f32 + GRID_GAP * (grid.count as f32 - 1.0);
        assert!((used - available).abs() < 0.01, "{available}");
        assert!(grid.width >= GRID_MIN_COLUMN, "{available}");
    }
}

#[test]
fn wider_windows_show_more_covers_without_a_cap() {
    assert!(grid_columns(3000.0).count > grid_columns(1600.0).count);
    assert!(grid_columns(7000.0).count > grid_columns(3000.0).count);
}

#[test]
fn tiny_and_zero_widths_keep_two_columns_without_negative_sizes() {
    assert_eq!(grid_columns(0.0).count, 2);
    assert_eq!(grid_columns(0.0).width, 0.0);
    assert_eq!(grid_columns(100.0).count, 2);
    assert_eq!(grid_columns(100.0).width, 40.0);
    assert_eq!(grid_columns(-50.0).width, 0.0);
}
```

- [ ] **步骤 2：实现列网格**

`src/app/style/grid.rs`：

```rust
//! The shared cover column grid.

pub(in crate::app) const GRID_MIN_COLUMN: f32 = 160.0;
pub(in crate::app) const GRID_GAP: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::app) struct GridColumns {
    pub(in crate::app) count: usize,
    pub(in crate::app) width: f32,
}

/// As many columns as fit at the minimum width, stretched to fill exactly, so
/// every shelf shares its left and right edges with the row above it.
pub(in crate::app) fn grid_columns(available: f32) -> GridColumns {
    let available = available.max(0.0);
    let count = (((available + GRID_GAP) / (GRID_MIN_COLUMN + GRID_GAP)).floor() as usize).max(2);
    let width = ((available - (count as f32 - 1.0) * GRID_GAP) / count as f32).max(0.0);
    GridColumns { count, width }
}

#[cfg(test)]
mod tests;
```

在 `src/app/style.rs` 加 `pub(in crate::app) mod grid;`，并用 `cargo test --locked -j 8 style::grid` 确认通过。此时 `grid_columns` 只有测试在用，先不跑 clippy，下面几步接上。

- [ ] **步骤 3：写封面铺底的失败测试**

`src/app/style/cover_tint/tests.rs`：

```rust
use gpui_kit::hsla;

use super::*;
use crate::app::covers::artwork::contrast_ratio;
use crate::app::style::tokens::{DARK, LIGHT};

const SURFACES: [TintSurface; 4] = [
    TintSurface::HomeFeature,
    TintSurface::DetailHeader,
    TintSurface::NowPlaying,
    TintSurface::MiniPlayer,
];

#[test]
fn text_stays_readable_on_every_cover_color() {
    for (palette, dark) in [(LIGHT.resolve(), false), (DARK.resolve(), true)] {
        for surface in SURFACES {
            for hue in (0..36).map(|step| step as f32 / 36.0) {
                for saturation in [0.0, 0.3, 0.6, 1.0] {
                    for lightness in [0.02, 0.5, 0.98] {
                        let color = hsla(hue, saturation, lightness, 1.0);
                        let (start, end) =
                            cover_tint(surface, Some((color, color)), palette.page, dark).unwrap();
                        for background in [start, end] {
                            for text in [palette.text, palette.text_secondary] {
                                let ratio = contrast_ratio(text, background);
                                assert!(
                                    ratio >= 4.5,
                                    "{surface:?} dark={dark} h={hue} s={saturation} l={lightness}: {ratio:.2}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn missing_palette_produces_no_tint() {
    let page = LIGHT.resolve().page;
    assert!(cover_tint(TintSurface::DetailHeader, None, page, false).is_none());
    assert!(cover_tint(TintSurface::NowPlaying, None, page, true).is_none());
}

#[test]
fn hue_survives_normalization() {
    let page = LIGHT.resolve().page;
    let red = hsla(0.0, 1.0, 0.05, 1.0);
    let (start, _) = cover_tint(TintSurface::NowPlaying, Some((red, red)), page, false).unwrap();
    let rgb = start.to_rgb();
    assert!(rgb.r > rgb.g && rgb.r > rgb.b);
}

#[test]
fn grey_covers_stay_neutral() {
    let page = LIGHT.resolve().page;
    let grey = hsla(0.0, 0.0, 0.5, 1.0);
    let (start, _) = cover_tint(TintSurface::NowPlaying, Some((grey, grey)), page, false).unwrap();
    let (tinted, base) = (start.to_rgb(), page.to_rgb());
    assert!(tinted.r < base.r);
}
```

- [ ] **步骤 4：实现封面铺底**

`src/app/style/cover_tint.rs`：

```rust
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
        if dark { DARK_LIGHTNESS } else { LIGHT_LIGHTNESS },
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
```

在 `src/app/style.rs` 加 `pub(in crate::app) mod cover_tint;`。

运行：`cargo test --locked -j 8 style::cover_tint`

预期：4 个测试通过。遍历测试失败时，降低失败表面的强度，不要放宽 4.5 的阈值。

- [ ] **步骤 5：写问候语（测试先行）**

`src/app/home/greeting/tests.rs`：

```rust
use super::*;

#[test]
fn hour_boundaries_match_the_spec() {
    assert_eq!(greeting_for_hour(4), Greeting::Evening);
    assert_eq!(greeting_for_hour(5), Greeting::Morning);
    assert_eq!(greeting_for_hour(11), Greeting::Morning);
    assert_eq!(greeting_for_hour(12), Greeting::Afternoon);
    assert_eq!(greeting_for_hour(17), Greeting::Afternoon);
    assert_eq!(greeting_for_hour(18), Greeting::Evening);
    assert_eq!(greeting_for_hour(23), Greeting::Evening);
    assert_eq!(greeting_for_hour(0), Greeting::Evening);
}
```

`src/app/home/greeting.rs`：

```rust
//! The Home page's time-of-day heading.

use chrono::Timelike;
use rust_i18n::t;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Greeting {
    Morning,
    Afternoon,
    Evening,
}

fn greeting_for_hour(hour: u32) -> Greeting {
    match hour {
        5..=11 => Greeting::Morning,
        12..=17 => Greeting::Afternoon,
        _ => Greeting::Evening,
    }
}

pub(in crate::app) fn current_greeting() -> std::borrow::Cow<'static, str> {
    match greeting_for_hour(chrono::Local::now().hour()) {
        Greeting::Morning => t!("Good morning"),
        Greeting::Afternoon => t!("Good afternoon"),
        Greeting::Evening => t!("Good evening"),
    }
}

#[cfg(test)]
mod tests;
```

`src/app/home.rs` 加 `pub(in crate::app) mod greeting;`。

三个语言文件各加（en / zh-CN / zh-TW 依次），放在字母序位置：

```yaml
"Good afternoon": "Good afternoon"
"Good evening": "Good evening"
"Good morning": "Good morning"
```
```yaml
"Good afternoon": "下午好"
"Good evening": "晚上好"
"Good morning": "早上好"
```
```yaml
"Good afternoon": "午安"
"Good evening": "晚安"
"Good morning": "早安"
```

- [ ] **步骤 6：写货架组件和取色入口**

`src/app/prelude.rs` 把 `style::buttons` 和 `style::surface` 两行改成：

```rust
pub(in crate::app) use crate::app::style::buttons::{
    brand_button_style, capsule_button, round_play_button, soft_toggle_style,
};
pub(in crate::app) use crate::app::style::surface::{cover_shadow, raised_surface};
```

`src/app/widgets/shelf.rs`：

```rust
//! Shelves: the page is a stack of them, so new Home features add a shelf
//! rather than a rearrangement. Implemented presentations are large cards and
//! square covers; compact song rows and chip rows join when content needs them.

use crate::app::prelude::*;
use crate::app::style::grid::{GridColumns, GRID_GAP};

/// Square covers on the shared column grid.
pub(in crate::app) fn cover_grid(
    columns: GridColumns,
    items: impl IntoIterator<Item = gpui_kit::AnyElement>,
) -> gpui_kit::Div {
    div()
        .w_full()
        .grid()
        .grid_cols(columns.count as u16)
        .gap(px(GRID_GAP))
        .children(items)
}

/// Large cards sharing a row equally.
pub(in crate::app) fn hero_shelf(
    cards: impl IntoIterator<Item = gpui_kit::AnyElement>,
) -> gpui_kit::Div {
    h_flex()
        .w_full()
        .gap(px(GRID_GAP))
        .items_stretch()
        .children(cards.into_iter().map(|card| div().flex_1().min_w_0().child(card)))
}

pub(in crate::app) fn shelf_section(
    title: gpui_kit::AnyElement,
    content: impl IntoElement,
) -> gpui_kit::AnyElement {
    v_flex().gap_3().child(title).child(content).into_any_element()
}
```

`src/app/widgets.rs` 加 `pub(in crate::app) mod shelf;`。

`src/app/style/tokens.rs` 加：

```rust
pub(in crate::app) const SECTION_GAP: f32 = 40.0;
pub(in crate::app) const CARD_TITLE: f32 = 26.0;
pub(in crate::app) const CARD_TITLE_COMPACT: f32 = 22.0;
pub(in crate::app) const HERO_CARD_HEIGHT: f32 = 176.0;
pub(in crate::app) const HERO_CARD_COMPACT_HEIGHT: f32 = 144.0;
pub(in crate::app) const HERO_COVER: f32 = 128.0;
pub(in crate::app) const HERO_COVER_COMPACT: f32 = 96.0;
pub(in crate::app) const HOVER_PLAY_BUTTON: f32 = 40.0;
```

`src/app/shell/layout.rs` 的 `impl NavidromeApp` 里加：

```rust
    pub(in crate::app) fn content_columns(&self, window: &Window) -> GridColumns {
        grid_columns(self.shell_layout(window).content_width)
    }
```

并在文件顶部加 `use crate::app::style::grid::{grid_columns, GridColumns};`。

`src/app/player/controls.rs` 的 `impl NavidromeApp` 里加（放在 `now_playing_palette` 之前）：

```rust
    /// Cover color for a content surface; `None` until the palette has loaded.
    pub(in crate::app) fn tint_for_cover(
        &self,
        surface: TintSurface,
        cover_id: Option<&str>,
        cx: &Context<Self>,
    ) -> Option<(Hsla, Hsla)> {
        let palette = cover_id.and_then(|id| self.state.covers.palette(id));
        cover_tint(surface, palette, cx.theme().background, cx.theme().is_dark())
    }
```

文件顶部加 `use crate::app::style::cover_tint::{cover_tint, TintSurface};`。

- [ ] **步骤 7：重写专辑卡片和骨架屏**

`src/app/library/album_grid.rs` 整个文件替换为：

```rust
//! Album artwork cards, playback actions and metadata links.

use crate::app::prelude::*;
use crate::app::shell::view_helpers::item_instance_key;
use crate::app::widgets::shelf::cover_grid;

impl NavidromeApp {
    pub(in crate::app) fn render_album_grid(
        &self,
        scope: &str,
        albums: &[Album],
        cx: &Context<Self>,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        let grid_start = std::time::Instant::now();
        let columns = self.content_columns(window);
        let cards = albums
            .iter()
            .enumerate()
            .map(|(index, album)| self.render_album_card(scope, index, album, columns.width, cx, window))
            .collect::<Vec<_>>();
        let grid = cover_grid(columns, cards);
        let grid_elapsed = grid_start.elapsed();
        if grid_elapsed > Duration::from_millis(8) {
            log::warn!(
                "render_album_grid built {} albums in {grid_elapsed:?}",
                albums.len()
            );
        }
        grid.into_any_element()
    }

    fn render_album_card(
        &self,
        scope: &str,
        index: usize,
        album: &Album,
        width: f32,
        cx: &Context<Self>,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let key = item_instance_key(scope, "album", index, &album.id);
        let hovered = self.state.hovered_item.as_deref() == Some(key.as_str());
        let album_for_click = album.clone();
        let album_for_play = album.clone();
        let album_for_cover = album.clone();
        let album_id_for_hover = key.clone();
        v_flex()
            .id(SharedString::from(key))
            .min_w_0()
            .gap_1()
            .child(
                div()
                    .id(SharedString::from(format!("album-cover-{}", album.id)))
                    .relative()
                    .w(px(width))
                    .h(px(width))
                    .mb_1()
                    .cursor_pointer()
                    .on_hover(cx.listener(move |this, hovering, _, cx| {
                        this.set_hovered(&album_id_for_hover, *hovering, cx);
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_album(album_for_cover.clone());
                        cx.notify();
                    }))
                    .child(self.render_cover_styled(
                        album.cover_art.as_deref(),
                        width,
                        tokens::radius::COVER,
                        cover_shadow(hovered, cx),
                    ))
                    .child(
                        div()
                            .absolute()
                            .top_2()
                            .right_2()
                            .rounded_full()
                            .bg(p.raised.opacity(0.72))
                            .child(self.favorite_button(FavoriteKind::Album, &album.id, cx)),
                    )
                    .when(hovered, |this| {
                        this.child(
                            div()
                                .absolute()
                                .right_2()
                                .bottom_2()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(
                                    round_play_button(
                                        SharedString::from(format!("album-play-{}", album.id)),
                                        PlayerIcon::Play,
                                        t!("Play album"),
                                        tokens::HOVER_PLAY_BUTTON,
                                        cx,
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.play_album(album_for_play.clone());
                                        cx.notify();
                                    })),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .id(SharedString::from(format!("album-{}", album.id)))
                    .min_w_0()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_album(album_for_click.clone());
                        cx.notify();
                    }))
                    .child(self.render_scrolling_title(
                        album.name.clone().into(),
                        width,
                        SharedString::from(format!("album-title-{}", album.id)),
                        window,
                    )),
            )
            .child(
                h_flex()
                    .text_size(px(tokens::META))
                    .text_color(p.text_secondary)
                    .min_w_0()
                    .gap_1()
                    .child(self.artist_name_link(
                        SharedString::from(format!("album-grid-artist-{}", album.id)),
                        album.artist.clone(),
                        album.artist_id.clone(),
                        album.cover_art.clone(),
                        cx,
                    ))
                    .when_some(album.year, |this, year| {
                        this.child(t!("- %{year}", year = year))
                    }),
            )
            .into_any_element()
    }
}
```

`src/app/widgets/skeleton.rs` 的 `render_album_skeleton_grid` 替换为（列表骨架和艺术家骨架留到 Task 5）：

```rust
    /// 专辑网格骨架屏：数据加载期间占位，避免空页面跳动。
    pub(in crate::app) fn render_album_skeleton_grid(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let columns = self.content_columns(window);
        let sunken = palette(cx).sunken;
        cover_grid(
            columns,
            (0..columns.count * 2).map(|_| {
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .w(px(columns.width))
                            .h(px(columns.width))
                            .rounded(px(tokens::radius::COVER))
                            .bg(sunken),
                    )
                    .child(div().w(relative(0.7)).h(px(12.0)).rounded_full().bg(sunken))
                    .child(div().w(relative(0.5)).h(px(10.0)).rounded_full().bg(sunken))
                    .into_any_element()
            }),
        )
        .into_any_element()
    }
```

文件顶部加 `use crate::app::widgets::shelf::cover_grid;`。

调用方 `src/app/library/browse_view.rs`、`src/app/library/detail_view.rs`、`src/app/home/section_view.rs` 里的 `self.render_album_skeleton_grid(cx)` 都改成 `self.render_album_skeleton_grid(window, cx)`。这几处都已经有 `window` 变量。

- [ ] **步骤 8：重写首页主卡片**

`src/app/home/feature_view.rs` 整个文件替换为：

```rust
//! Home listening feature: resume the current song, or else the latest album.

use crate::app::prelude::*;
use crate::app::style::cover_tint::{tint_background, TintSurface};

#[derive(Clone)]
enum FeatureTarget {
    NowPlaying,
    Album(Album),
}

impl NavidromeApp {
    fn open_feature(&mut self, target: &FeatureTarget) {
        match target {
            FeatureTarget::NowPlaying => self.open_now_playing(),
            FeatureTarget::Album(album) => self.open_album(album.clone()),
        }
    }

    fn play_feature(&mut self, target: &FeatureTarget) {
        match target {
            FeatureTarget::NowPlaying => self.toggle_playback(),
            FeatureTarget::Album(album) => self.play_album(album.clone()),
        }
    }

    /// Home 的主焦点：优先继续当前播放；无播放会话时展示最近专辑。
    pub(in crate::app) fn render_home_feature(
        &self,
        compact: bool,
        cx: &Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        let (target, ids, eyebrow, title, subtitle, cover_id, play_icon, play_label, open_label) =
            if let Some(song) = &self.state.now_playing {
                let playback = self.audio.state();
                let is_playing = playback.active && !playback.paused;
                (
                    FeatureTarget::NowPlaying,
                    (
                        "home-continue-listening",
                        "home-feature-cover",
                        "home-feature-playback",
                        "home-feature-now-playing",
                    ),
                    t!("Continue listening"),
                    song.title.clone(),
                    if song.album.trim().is_empty() {
                        song.artist.clone()
                    } else {
                        format!("{} · {}", song.artist, song.album)
                    },
                    song.cover_art.clone(),
                    if is_playing {
                        PlayerIcon::Pause
                    } else {
                        PlayerIcon::Play
                    },
                    if is_playing { t!("Pause") } else { t!("Play") },
                    t!("Open player"),
                )
            } else {
                let album = self
                    .state
                    .home_albums
                    .albums(HomeAlbumSection::Recent)
                    .first()?
                    .clone();
                (
                    FeatureTarget::Album(album.clone()),
                    (
                        "home-recent-feature",
                        "home-feature-album-cover",
                        "home-feature-play-album",
                        "home-feature-open-album",
                    ),
                    t!("Recently played"),
                    album.name.clone(),
                    album.artist.clone(),
                    album.cover_art.clone(),
                    PlayerIcon::Play,
                    t!("Play album"),
                    t!("Go to album"),
                )
            };
        let (container_id, cover_element_id, play_id, open_id) = ids;
        let p = palette(cx);
        let tint = self.tint_for_cover(TintSurface::HomeFeature, cover_id.as_deref(), cx);
        let art_size = if compact {
            tokens::HERO_COVER_COMPACT
        } else {
            tokens::HERO_COVER
        };
        let (target_cover, target_play, target_open) =
            (target.clone(), target.clone(), target);

        Some(
            h_flex()
                .id(container_id)
                .w_full()
                .h(px(if compact {
                    tokens::HERO_CARD_COMPACT_HEIGHT
                } else {
                    tokens::HERO_CARD_HEIGHT
                }))
                .when(compact, |this| this.p_4().gap_4())
                .when(!compact, |this| this.p_6().gap_6())
                .items_center()
                .rounded(px(tokens::radius::LARGE))
                .bg(tint_background(tint, p.sunken, 118.0))
                .child(
                    div()
                        .id(cover_element_id)
                        .flex_none()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_feature(&target_cover);
                            cx.notify();
                        }))
                        .child(self.render_cover_styled(
                            cover_id.as_deref(),
                            art_size,
                            tokens::radius::COVER,
                            cover_shadow(false, cx),
                        )),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(tokens::META))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.text_secondary)
                                .child(eyebrow),
                        )
                        .child(
                            serif_heading(
                                title,
                                if compact {
                                    tokens::CARD_TITLE_COMPACT
                                } else {
                                    tokens::CARD_TITLE
                                },
                                cx,
                            )
                            .truncate(),
                        )
                        .child(
                            div()
                                .text_size(px(tokens::META))
                                .text_color(p.text_secondary)
                                .truncate()
                                .child(subtitle),
                        )
                        .child(
                            h_flex()
                                .pt_2()
                                .gap_2()
                                .items_center()
                                .child(
                                    round_play_button(
                                        play_id,
                                        play_icon,
                                        play_label,
                                        if compact {
                                            tokens::PLAY_BUTTON_COMPACT
                                        } else {
                                            tokens::PLAY_BUTTON
                                        },
                                        cx,
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.play_feature(&target_play);
                                        cx.notify();
                                    })),
                                )
                                .child(
                                    Button::new(open_id)
                                        .label(open_label)
                                        .icon(AppIcon::ChevronRight)
                                        .ghost()
                                        .small()
                                        .rounded(px(tokens::radius::CONTROL))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.open_feature(&target_open);
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}
```

- [ ] **步骤 9：重写每日精选卡片**

`src/app/recommendations/daily_mix_view.rs` 整个文件替换为：

```rust
//! Home Daily Mix card and its playback, refresh and append controls.

use crate::app::prelude::*;

impl NavidromeApp {
    /// Today's first four covers; missing slots show the default cover.
    fn render_daily_mix_mosaic(&self, size: f32) -> gpui_kit::AnyElement {
        let gap = 4.0;
        let cell = (size - gap) / 2.0;
        let covers = self
            .state
            .daily_mix
            .snapshot()
            .map(|snapshot| {
                snapshot
                    .tracks
                    .iter()
                    .filter_map(|track| track.song.cover_art.as_deref())
                    .take(4)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        div()
            .flex_none()
            .w(px(size))
            .h(px(size))
            .grid()
            .grid_cols(2)
            .gap(px(gap))
            .children((0..4).map(|index| {
                self.render_cover_styled(
                    covers.get(index).copied(),
                    cell,
                    tokens::radius::CONTROL,
                    Vec::new(),
                )
            }))
            .into_any_element()
    }

    pub(in crate::app) fn render_daily_mix_card(
        &self,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let loading = self.state.daily_mix.loading();
        let error = self.state.daily_mix.error();
        let count = self
            .state
            .daily_mix
            .snapshot()
            .map_or(0, |snapshot| snapshot.tracks.len());
        let description = if loading {
            t!("Creating your daily mix…")
        } else if let Some(error) = error {
            t!("Daily mix unavailable: %{error}", error = error)
        } else {
            t!("A balanced mix of favorites, familiar artists, and new discoveries.")
        };
        let title = if count > 0 {
            t!("%{count} tracks selected for today", count = count)
        } else {
            t!("Daily Mix")
        };
        raised_surface(
            h_flex()
                .w_full()
                .h(px(if compact {
                    tokens::HERO_CARD_COMPACT_HEIGHT
                } else {
                    tokens::HERO_CARD_HEIGHT
                }))
                .when(compact, |this| this.p_4().gap_4())
                .when(!compact, |this| this.p_6().gap_6())
                .items_center(),
            tokens::radius::LARGE,
            cx,
        )
        .child(self.render_daily_mix_mosaic(if compact {
            tokens::HERO_COVER_COMPACT
        } else {
            tokens::HERO_COVER
        }))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .when(count > 0, |this| {
                    this.child(
                        div()
                            .text_size(px(tokens::META))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text_secondary)
                            .child(t!("Daily Mix")),
                    )
                })
                .child(
                    serif_heading(
                        title,
                        if compact {
                            tokens::CARD_TITLE_COMPACT
                        } else {
                            tokens::CARD_TITLE
                        },
                        cx,
                    )
                    .truncate(),
                )
                .child(
                    div()
                        .text_size(px(tokens::META))
                        .truncate()
                        .text_color(if error.is_some() {
                            cx.theme().danger
                        } else {
                            p.text_secondary
                        })
                        .child(description),
                )
                .child(
                    h_flex()
                        .pt_2()
                        .gap_2()
                        .items_center()
                        .when(count == 0, |this| {
                            this.child(
                                Button::new("daily-mix-create")
                                    .label(if loading {
                                        t!("Creating…")
                                    } else if error.is_some() {
                                        t!("Retry")
                                    } else {
                                        t!("Create and play")
                                    })
                                    .custom(brand_button_style(cx))
                                    .rounded(px(tokens::radius::FULL))
                                    .small()
                                    .disabled(loading)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if this.state.daily_mix.error().is_some() {
                                            this.retry_daily_mix();
                                        } else {
                                            this.play_daily_mix();
                                        }
                                        cx.notify();
                                    })),
                            )
                        })
                        .when(count > 0, |this| {
                            this.child(
                                round_play_button(
                                    "daily-mix-play",
                                    PlayerIcon::Play,
                                    t!("Play"),
                                    if compact {
                                        tokens::PLAY_BUTTON_COMPACT
                                    } else {
                                        tokens::PLAY_BUTTON
                                    },
                                    cx,
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.play_daily_mix();
                                    cx.notify();
                                })),
                            )
                            .child(
                                capsule_button("daily-mix-append", cx)
                                    .icon(AppIcon::Queue)
                                    .tooltip(t!("Add to queue"))
                                    .small()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.append_daily_mix();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("daily-mix-refresh")
                                    .icon(AppIcon::Refresh)
                                    .tooltip(t!("Refresh mix"))
                                    .ghost()
                                    .small()
                                    .rounded(px(tokens::radius::CONTROL))
                                    .disabled(loading)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.refresh_daily_mix();
                                        cx.notify();
                                    })),
                            )
                        }),
                ),
        )
        .into_any_element()
    }
}
```

- [ ] **步骤 10：改首页组合和区块标题**

`src/app/home/view.rs` 整个文件替换为：

```rust
//! Home page composition: a greeting above a stack of shelves.

use super::greeting::current_greeting;
use crate::app::prelude::*;
use crate::app::widgets::shelf::hero_shelf;

impl NavidromeApp {
    pub(in crate::app) fn render_home(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let columns = self.content_columns(window).count;
        let compact = self.shell_layout(window).compact;
        let recent = self.state.home_albums.albums(HomeAlbumSection::Recent);
        let frequent = self.state.home_albums.albums(HomeAlbumSection::Frequent);
        let newest = self.state.home_albums.albums(HomeAlbumSection::Newest);
        let random = self.state.home_albums.albums(HomeAlbumSection::Random);
        let feature_uses_recent = self.state.now_playing.is_none() && !recent.is_empty();
        let recent_start = usize::from(feature_uses_recent);
        let recent_end = (recent_start + columns).min(recent.len());
        let recent_preview = &recent[recent_start..recent_end];
        let frequent_preview = &frequent[..frequent.len().min(columns)];
        let newest_preview = &newest[..newest.len().min(columns)];
        let random_preview = &random[..random.len().min(columns)];
        let mut cards = Vec::with_capacity(2);
        cards.extend(self.render_home_feature(compact, cx));
        cards.push(self.render_daily_mix_card(compact, cx));

        v_flex()
            .gap(px(tokens::SECTION_GAP))
            .child(
                v_flex()
                    .gap_6()
                    .child(serif_heading(current_greeting(), tokens::PAGE_TITLE, cx))
                    .child(hero_shelf(cards)),
            )
            .when(
                !recent_preview.is_empty()
                    || self.state.home_albums.loading(HomeAlbumSection::Recent),
                |this| {
                    this.child(self.render_home_primary_section(
                        t!("Recently played"),
                        self.render_home_album_content(
                            "recent",
                            recent_preview,
                            self.state.home_albums.loading(HomeAlbumSection::Recent),
                            t!("Nothing played yet — recently played albums will appear here."),
                            cx,
                            window,
                        ),
                        cx,
                    ))
                },
            )
            .child(self.render_home_section(
                "frequent",
                t!("Frequently played"),
                || {
                    self.render_home_album_content(
                    "frequent",
                    frequent_preview,
                    self.state.home_albums.loading(HomeAlbumSection::Frequent),
                    t!("Nothing frequently played yet — your most-played albums will appear here."),
                    cx,
                    window,
                )
                },
                cx,
            ))
            .child(self.render_home_section(
                "newest",
                t!("Newest albums"),
                || {
                    self.render_home_album_content(
                        "newest",
                        newest_preview,
                        self.state.home_albums.loading(HomeAlbumSection::Newest),
                        t!("No albums in your library yet."),
                        cx,
                        window,
                    )
                },
                cx,
            ))
            .child(self.render_home_section(
                "random",
                t!("Random albums"),
                || {
                    self.render_home_album_content(
                        "random",
                        random_preview,
                        self.state.home_albums.loading(HomeAlbumSection::Random),
                        t!("No random albums to show — refresh the library for a fresh pick."),
                        cx,
                        window,
                    )
                },
                cx,
            ))
            .into_any_element()
    }
}
```

`src/app/home/section_view.rs`：

把 `render_home_section` 里的标题行替换为（去掉横线，箭头紧跟标题）：

```rust
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .id(SharedString::from(format!("home-section-{id}")))
                    .hover(|style| style.text_color(cx.theme().primary))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_home_section(id, cx);
                    }))
                    .child(section_title(title.clone(), cx))
                    .child(
                        Icon::new(if collapsed {
                            AppIcon::ChevronRight
                        } else {
                            AppIcon::ChevronDown
                        })
                        .size(px(15.0))
                        .text_color(palette(cx).text_muted),
                    ),
            )
```

把 `render_home_primary_section` 替换为：

```rust
    pub(in crate::app) fn render_home_primary_section(
        &self,
        title: impl Into<SharedString>,
        content: gpui_kit::AnyElement,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        shelf_section(section_title(title, cx).into_any_element(), content)
    }
```

文件顶部加 `use crate::app::widgets::shelf::shelf_section;`。

`src/app/player/controls.rs`：`home_feature_colors` 已随 `feature_view.rs` 重写而消失；`src/app/home/feature_view.rs` 和 `daily_mix_view.rs` 不再导入 `covers::artwork`。

- [ ] **步骤 11：运行标准验证**

预期：全部通过。

- [ ] **步骤 12：请用户查看**

告诉用户：看首页。检查问候语、两张等宽大卡片、封面随窗口变宽而增多且上下对齐（全屏、拖窄、开关队列抽屉各看一次）、悬停时右下角的玫瑰红播放钮、折叠箭头，以及没有正在播放时的最近专辑卡。

---

### Task 5: 媒体库网格页

**Files:**
- Create: `src/app/style/genre_color.rs`、`src/app/style/genre_color/tests.rs`、`src/app/playlists/list_view/tests.rs`
- Modify: `src/app/style.rs`、`src/app/library/artist_grid.rs`、`src/app/library/browse_view.rs`、`src/app/playlists/list_view.rs`、`src/app/widgets/skeleton.rs`、`src/app/shell/content.rs`、`locales/*.yml`

**Interfaces:**
- Consumes: `cover_grid`、`shelf_section`、`content_columns`、`render_cover_styled`、`round_play_button`。
- Produces: `genre_color::genre_swatch(name: &str, dark: bool) -> Hsla`；`render_artist_skeleton_grid(window, cx)`；`render_playlists(window, cx)`。

- [ ] **步骤 1：写流派配色的失败测试**

`src/app/style/genre_color/tests.rs`：

```rust
use super::*;
use crate::app::covers::artwork::contrast_ratio;
use crate::app::style::tokens::{DARK, LIGHT};

#[test]
fn same_name_always_gets_the_same_color() {
    assert_eq!(genre_swatch("Rock", false), genre_swatch("Rock", false));
    assert_eq!(genre_swatch("摇滚", true), genre_swatch("摇滚", true));
}

#[test]
fn names_spread_across_the_swatches() {
    let names = [
        "Rock", "Pop", "Jazz", "Blues", "Folk", "Metal", "Classical", "Electronic", "Hip-Hop",
        "Country", "Soul", "Funk", "Reggae", "Punk", "摇滚", "流行", "民谣", "古典",
    ];
    let mut used = std::collections::HashSet::new();
    for name in names {
        used.insert(swatch_index(name));
    }
    assert!(used.len() >= 5, "only {} swatches used", used.len());
}

#[test]
fn every_swatch_is_readable() {
    let light_text = LIGHT.resolve();
    for swatch in LIGHT_SWATCHES {
        let color: Hsla = gpui_kit::rgb(swatch).into();
        assert!(contrast_ratio(light_text.text, color) >= 4.5);
        assert!(contrast_ratio(light_text.text_secondary, color) >= 4.5);
    }
    let dark_text = DARK.resolve();
    for swatch in DARK_SWATCHES {
        let color: Hsla = gpui_kit::rgb(swatch).into();
        assert!(contrast_ratio(dark_text.text, color) >= 4.5);
        assert!(contrast_ratio(dark_text.text_secondary, color) >= 4.5);
    }
}

#[test]
fn empty_name_does_not_panic() {
    let _ = genre_swatch("", false);
}
```

- [ ] **步骤 2：实现流派配色**

`src/app/style/genre_color.rs`：

```rust
//! Name-derived color blocks for genre cards.

use gpui_kit::{rgb, Hsla};

const LIGHT_SWATCHES: [u32; 8] = [
    0xF3D9CF, 0xF2DDE3, 0xE3DDF0, 0xD8E3EF, 0xD6E8E2, 0xDFE8D2, 0xEFE6C9, 0xEBDCD0,
];

const DARK_SWATCHES: [u32; 8] = [
    0x4A3128, 0x4A2F39, 0x3A3249, 0x2C3A49, 0x2A403A, 0x35402A, 0x453D24, 0x43352A,
];

/// FNV-1a: unlike `DefaultHasher` it never changes between Rust releases, so a
/// genre keeps its color across updates.
fn swatch_index(name: &str) -> usize {
    let hash = name.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    hash as usize % LIGHT_SWATCHES.len()
}

pub(in crate::app) fn genre_swatch(name: &str, dark: bool) -> Hsla {
    let swatches = if dark { DARK_SWATCHES } else { LIGHT_SWATCHES };
    rgb(swatches[swatch_index(name)]).into()
}

#[cfg(test)]
mod tests;
```

`src/app/style.rs` 加 `pub(in crate::app) mod genre_color;`。

运行：`cargo test --locked -j 8 style::genre_color`

预期：4 个测试通过。

- [ ] **步骤 3：写播放列表副标题的失败测试**

`src/app/playlists/list_view/tests.rs`：

```rust
use super::*;

fn subtitle(duration: Option<i32>, owner: Option<&str>) -> String {
    playlist_card_subtitle(12, duration, owner)
}

#[test]
fn own_playlist_shows_count_and_duration() {
    let text = subtitle(Some(3900), None);
    assert!(text.contains("12"), "{text}");
    assert!(text.contains(&format_duration(Some(3900))), "{text}");
}

#[test]
fn unknown_duration_is_omitted() {
    let text = subtitle(None, None);
    assert!(text.contains("12"), "{text}");
    assert!(!text.contains('·'), "{text}");
}

#[test]
fn shared_playlist_names_its_owner() {
    let text = subtitle(Some(60), Some("alice"));
    assert!(text.contains("alice"), "{text}");
}
```

- [ ] **步骤 4：把播放列表页改成封面网格**

`src/app/playlists/list_view.rs` 顶部加 `use crate::app::widgets::shelf::cover_grid;`。

把 `render_playlists` 的签名改成 `(&self, window: &mut Window, cx: &Context<Self>)`，开头的 `let playlist_list = ...;` 整段替换为：

```rust
        let columns = self.content_columns(window);
        let p = palette(cx);
        let playlist_list = cover_grid(
            columns,
            self.state
                .playlists
                .visible_items()
                .iter()
                .map(|playlist| {
                    let playlist_for_click = playlist.clone();
                    let shared_owner = (!crate::playlist::can_edit(playlist, &self.config.username))
                        .then(|| playlist.owner.as_deref())
                        .flatten();
                    let details = playlist_card_subtitle(
                        playlist.song_count.unwrap_or_default(),
                        playlist.duration,
                        shared_owner,
                    );
                    v_flex()
                        .id(SharedString::from(format!("playlist-row-{}", playlist.id)))
                        .min_w_0()
                        .gap_1()
                        .cursor_pointer()
                        .child(div().mb_1().child(self.render_cover_styled(
                            playlist.cover_art.as_deref(),
                            columns.width,
                            tokens::radius::COVER,
                            cover_shadow(false, cx),
                        )))
                        .child(
                            div()
                                .text_size(px(tokens::BODY))
                                .font_weight(FontWeight::MEDIUM)
                                .truncate()
                                .child(playlist.name.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(tokens::META))
                                .text_color(p.text_secondary)
                                .truncate()
                                .child(details),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_playlist(playlist_for_click.clone());
                            cx.notify();
                        }))
                        .into_any_element()
                })
                .collect::<Vec<_>>(),
        );
```

把函数里的 `self.render_list_skeleton(8, cx)` 改成 `self.render_album_skeleton_grid(window, cx)`，`.gap_5()` 改成 `.gap_6()`。文件末尾（`impl` 之后）加：

```rust
fn playlist_card_subtitle(track_count: i32, duration: Option<i32>, shared_owner: Option<&str>) -> String {
    let mut parts = vec![t!("%{track_count} tracks", track_count = track_count).to_string()];
    if duration.is_some() {
        parts.push(format_duration(duration));
    }
    if let Some(owner) = shared_owner {
        parts.push(t!("Shared").to_string());
        parts.push(owner.to_string());
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests;
```

`src/app/shell/content.rs` 的 `View::Playlists => self.render_playlists(cx),` 改成 `View::Playlists => self.render_playlists(window, cx),`。

三个语言文件加 `"Shared"`（en: `Shared`，zh-CN: `共享`，zh-TW: `共享`），并删除不再引用的 `"%{track_count} tracks | %{owner}"` 一行。

运行：`cargo test --locked -j 8 playlists::list_view`

预期：3 个测试通过。

- [ ] **步骤 5：重写艺术家卡片和骨架屏**

`src/app/library/artist_grid.rs` 整个文件替换为：

```rust
//! Artist artwork cards and library actions.

use crate::app::prelude::*;
use crate::app::shell::view_helpers::item_instance_key;
use crate::app::widgets::shelf::cover_grid;

impl NavidromeApp {
    pub(in crate::app) fn render_artist_grid(
        &self,
        artists: &[Artist],
        cx: &Context<Self>,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        let columns = self.content_columns(window);
        let p = palette(cx);
        let cards = artists
            .iter()
            .enumerate()
            .map(|(index, artist)| {
                let width = columns.width;
                let key = item_instance_key("artists", "artist", index, &artist.id);
                let hovered = self.state.hovered_item.as_deref() == Some(key.as_str());
                let artist_for_click = artist.clone();
                let artist_for_play = artist.clone();
                let artist_for_cover = artist.clone();
                let artist_id_for_hover = key.clone();
                v_flex()
                    .id(SharedString::from(key))
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .id(SharedString::from(format!("artist-cover-{}", artist.id)))
                            .relative()
                            .w(px(width))
                            .h(px(width))
                            .mb_1()
                            .cursor_pointer()
                            .on_hover(cx.listener(move |this, hovering, _, cx| {
                                this.set_hovered(&artist_id_for_hover, *hovering, cx);
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_artist(artist_for_cover.clone());
                                cx.notify();
                            }))
                            .child(self.render_cover_styled(
                                artist.cover_art.as_deref(),
                                width,
                                tokens::radius::FULL,
                                cover_shadow(hovered, cx),
                            ))
                            .child(
                                div()
                                    .absolute()
                                    .top_2()
                                    .right_2()
                                    .rounded_full()
                                    .bg(p.raised.opacity(0.72))
                                    .child(self.favorite_button(
                                        FavoriteKind::Artist,
                                        &artist.id,
                                        cx,
                                    )),
                            )
                            .when(hovered, |this| {
                                this.child(
                                    div()
                                        .absolute()
                                        .right_2()
                                        .bottom_2()
                                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .child(
                                            round_play_button(
                                                SharedString::from(format!(
                                                    "artist-play-{}",
                                                    artist.id
                                                )),
                                                PlayerIcon::Play,
                                                t!("Play all"),
                                                tokens::HOVER_PLAY_BUTTON,
                                                cx,
                                            )
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.play_artist(artist_for_play.clone());
                                                cx.notify();
                                            })),
                                        ),
                                )
                            }),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("artist-{}", artist.id)))
                            .min_w_0()
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_artist(artist_for_click.clone());
                                cx.notify();
                            }))
                            .child(self.render_scrolling_title(
                                artist.name.clone().into(),
                                width,
                                SharedString::from(format!("artist-title-{}", artist.id)),
                                window,
                            )),
                    )
                    .child(
                        div()
                            .text_size(px(tokens::META))
                            .text_color(p.text_secondary)
                            .child(
                                artist
                                    .album_count
                                    .map(|count| t!("%{count} albums", count = count))
                                    .unwrap_or_else(|| t!("Artist")),
                            ),
                    )
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        cover_grid(columns, cards).into_any_element()
    }
}
```

`src/app/widgets/skeleton.rs`：把 `render_artist_skeleton_grid` 替换为：

```rust
    /// 艺术家网格骨架屏：与艺术家页的圆形图片一致。
    pub(in crate::app) fn render_artist_skeleton_grid(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let columns = self.content_columns(window);
        let sunken = palette(cx).sunken;
        cover_grid(
            columns,
            (0..columns.count * 2).map(|_| {
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .w(px(columns.width))
                            .h(px(columns.width))
                            .rounded_full()
                            .bg(sunken),
                    )
                    .child(div().w(relative(0.6)).h(px(12.0)).rounded_full().bg(sunken))
                    .into_any_element()
            }),
        )
        .into_any_element()
    }
```

把 `render_list_skeleton` 里的 `let muted = cx.theme().muted_foreground;` 改成 `let sunken = palette(cx).sunken;`，三个 `.bg(muted.opacity(...))` 都改成 `.bg(sunken)`，`.rounded_md()` 改成 `.rounded(px(tokens::radius::CONTROL))`。

`src/app/library/browse_view.rs` 的 `self.render_artist_skeleton_grid(cx)` 改成 `self.render_artist_skeleton_grid(window, cx)`。

- [ ] **步骤 6：改网格页的分区和流派卡片**

`src/app/library/browse_view.rs`：

顶部加：

```rust
use crate::app::style::genre_color::genre_swatch;
use crate::app::widgets::shelf::{cover_grid, shelf_section};
```

收藏页：把 `let mut content = v_flex().gap_5().child(` 改成 `let mut content = v_flex().gap(px(tokens::SECTION_GAP)).child(`，并把三个“标题加内容”分别替换为 `shelf_section`：

```rust
                if !favorites.artists.is_empty() {
                    content = content.child(shelf_section(
                        section_title(t!("Artists"), cx).into_any_element(),
                        self.render_artist_grid(&favorites.artists, cx, window),
                    ));
                }
                if !favorites.albums.is_empty() {
                    content = content.child(shelf_section(
                        section_title(t!("Albums"), cx).into_any_element(),
                        self.render_album_grid("favorites", &favorites.albums, cx, window),
                    ));
                }
                if !favorites.songs.is_empty() {
                    content = content.child(shelf_section(
                        section_title(t!("Songs"), cx).into_any_element(),
                        self.render_song_list(SongScope::Favorites, cx),
                    ));
                }
```

其余分支（Artists、Albums、Genres、GenreDetail）的 `.gap_5()` 改成 `.gap_6()`。

`self.render_genre_grid(cx)` 改成 `self.render_genre_grid(window, cx)`，并把 `render_genre_grid` 替换为：

```rust
    /// 流派色块网格：同名流派永远同色，显示流派名与专辑/歌曲数，点击进入详情。
    fn render_genre_grid(&self, window: &Window, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let columns = self.content_columns(window);
        let p = palette(cx);
        let dark = cx.theme().is_dark();
        let genres = self.state.genres.items();
        cover_grid(
            columns,
            genres
                .iter()
                .enumerate()
                .map(|(index, genre)| {
                    let genre_for_click = genre.value.clone();
                    let counts = match (genre.album_count, genre.song_count) {
                        (Some(albums), Some(songs)) => t!(
                            "%{albums} albums · %{songs} songs",
                            albums = albums,
                            songs = songs
                        ),
                        (Some(albums), None) => t!("%{albums} albums", albums = albums),
                        (None, Some(songs)) => t!("%{songs} songs", songs = songs),
                        (None, None) => t!("—"),
                    };
                    v_flex()
                        .id(SharedString::from(format!("genre-card-{index}")))
                        .h(px(96.0))
                        .min_w_0()
                        .p_4()
                        .justify_end()
                        .gap_1()
                        .rounded(px(tokens::radius::COVER))
                        .bg(genre_swatch(&genre.value, dark))
                        .cursor_pointer()
                        .hover(|style| style.shadow(cover_shadow(true, cx)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_genre(genre_for_click.clone());
                            cx.notify();
                        }))
                        .child(
                            div()
                                .text_size(px(tokens::BODY))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.text)
                                .truncate()
                                .child(genre.value.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(tokens::CAPTION))
                                .text_color(p.text_secondary)
                                .truncate()
                                .child(counts),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>(),
        )
        .into_any_element()
    }
```

- [ ] **步骤 7：运行标准验证**

预期：全部通过。

- [ ] **步骤 8：请用户查看**

告诉用户：看艺术家（圆形）、专辑、流派（色块）、播放列表（封面网格）、收藏，确认全屏时边缘对齐，排序和刷新按钮还在。

---

### Task 6: 详情页头部与搜索页

**Files:**
- Create: `src/app/widgets/detail_hero.rs`、`src/app/widgets/detail_hero/tests.rs`
- Modify: `src/app/widgets.rs`、`src/app/style/tokens.rs`、`src/app/library/detail_view.rs`、`src/app/search/view.rs`、`src/app/shell/view_state.rs`、`src/app/shell/view_state/tests.rs`、`src/app/shell/render.rs`、`src/app/shell/content.rs`、`locales/*.yml`

**Interfaces:**
- Consumes: `tint_for_cover`、`tint_background`、`render_cover_styled`、`round_play_button`、`capsule_button`、`shelf_section`、`shell_layout`。
- Produces: `DetailHero { kind, title, cover_id, round_cover, meta, actions }`；`NavidromeApp::render_detail_hero(&self, hero: DetailHero, window: &Window, cx) -> AnyElement`；`View::has_detail_hero(self) -> bool`。

- [ ] **步骤 1：写失败测试**

`src/app/widgets/detail_hero/tests.rs`：

```rust
use super::*;

#[test]
fn large_window_uses_full_size_hero() {
    let sizes = hero_sizes(1280.0, 820.0, false);
    assert_eq!(sizes.cover, 220.0);
    assert_eq!(sizes.title, 40.0);
    assert_eq!(hero_sizes(1280.0, 820.0, true).cover, 150.0);
}

#[test]
fn narrow_or_short_window_shrinks_the_hero() {
    for (width, height) in [(1039.0, 820.0), (1280.0, 759.0), (900.0, 600.0)] {
        let sizes = hero_sizes(width, height, false);
        assert_eq!(sizes.cover, 140.0, "{width}x{height}");
        assert_eq!(sizes.title, 28.0, "{width}x{height}");
        assert_eq!(hero_sizes(width, height, true).cover, 110.0);
    }
}

#[test]
fn exact_thresholds_stay_full_size() {
    assert_eq!(hero_sizes(1040.0, 760.0, false).cover, 220.0);
}
```

`src/app/shell/view_state/tests.rs` 末尾加：

```rust
#[test]
fn only_album_artist_and_playlist_details_have_a_hero() {
    for view in VIEWS {
        let expected = matches!(
            view,
            View::ArtistDetail | View::AlbumDetail | View::PlaylistDetail
        );
        assert_eq!(view.has_detail_hero(), expected, "{view:?}");
    }
}
```

- [ ] **步骤 2：实现详情头部**

`src/app/style/tokens.rs` 加：

```rust
pub(in crate::app) const DETAIL_TITLE: f32 = 40.0;
pub(in crate::app) const DETAIL_TITLE_COMPACT: f32 = 28.0;
pub(in crate::app) const DETAIL_COVER: f32 = 220.0;
pub(in crate::app) const DETAIL_COVER_COMPACT: f32 = 140.0;
pub(in crate::app) const DETAIL_ARTIST_COVER: f32 = 150.0;
pub(in crate::app) const DETAIL_ARTIST_COVER_COMPACT: f32 = 110.0;
pub(in crate::app) const DETAIL_PLAY_BUTTON: f32 = 48.0;
pub(in crate::app) const DETAIL_COMPACT_WIDTH: f32 = 1040.0;
pub(in crate::app) const DETAIL_COMPACT_HEIGHT: f32 = 760.0;
```

`src/app/widgets.rs` 加 `pub(in crate::app) mod detail_hero;`。

`src/app/widgets/detail_hero.rs`：

```rust
//! The header shared by album, artist and playlist detail pages. It bleeds to
//! the content edges, so these pages pad the content below it themselves.

use crate::app::prelude::*;
use crate::app::style::cover_tint::{tint_background, TintSurface};

pub(in crate::app) struct DetailHero {
    pub(in crate::app) kind: SharedString,
    pub(in crate::app) title: SharedString,
    pub(in crate::app) cover_id: Option<String>,
    pub(in crate::app) round_cover: bool,
    pub(in crate::app) meta: gpui_kit::AnyElement,
    pub(in crate::app) actions: gpui_kit::AnyElement,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct HeroSizes {
    cover: f32,
    title: f32,
}

fn hero_sizes(viewport_width: f32, viewport_height: f32, round_cover: bool) -> HeroSizes {
    let compact =
        viewport_width < tokens::DETAIL_COMPACT_WIDTH || viewport_height < tokens::DETAIL_COMPACT_HEIGHT;
    HeroSizes {
        cover: match (round_cover, compact) {
            (false, false) => tokens::DETAIL_COVER,
            (false, true) => tokens::DETAIL_COVER_COMPACT,
            (true, false) => tokens::DETAIL_ARTIST_COVER,
            (true, true) => tokens::DETAIL_ARTIST_COVER_COMPACT,
        },
        title: if compact {
            tokens::DETAIL_TITLE_COMPACT
        } else {
            tokens::DETAIL_TITLE
        },
    }
}

impl NavidromeApp {
    pub(in crate::app) fn render_detail_hero(
        &self,
        hero: DetailHero,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let viewport = window.viewport_size();
        let sizes = hero_sizes(
            f32::from(viewport.width),
            f32::from(viewport.height),
            hero.round_cover,
        );
        let padding = self.shell_layout(window).page_padding;
        let tint = self
            .tint_for_cover(TintSurface::DetailHeader, hero.cover_id.as_deref(), cx)
            .map(|(start, _)| (start, cx.theme().background));
        h_flex()
            .w_full()
            .flex_none()
            .items_end()
            .gap_6()
            .p(px(padding))
            .bg(tint_background(tint, cx.theme().background, 180.0))
            .child(self.render_cover_styled(
                hero.cover_id.as_deref(),
                sizes.cover,
                if hero.round_cover {
                    tokens::radius::FULL
                } else {
                    tokens::radius::COVER
                },
                cover_shadow(false, cx),
            ))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(tokens::META))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text_secondary)
                            .child(hero.kind),
                    )
                    .child(serif_heading(hero.title, sizes.title, cx).truncate())
                    .child(
                        div()
                            .min_w_0()
                            .text_size(px(tokens::BODY))
                            .text_color(p.text_secondary)
                            .child(hero.meta),
                    )
                    .child(
                        h_flex()
                            .pt_2()
                            .gap_2()
                            .flex_wrap()
                            .items_center()
                            .child(hero.actions),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
```

`src/app/shell/view_state.rs` 的 `impl View` 里加：

```rust
    /// These pages open with a full-bleed header and pad their own content.
    pub(in crate::app) fn has_detail_hero(self) -> bool {
        matches!(
            self,
            Self::ArtistDetail | Self::AlbumDetail | Self::PlaylistDetail
        )
    }
```

`src/app/shell/render.rs`：把 `.p(px(layout.page_padding))` 改成：

```rust
                            .when(!self.state.view.has_detail_hero(), |this| {
                                this.p(px(layout.page_padding))
                            })
```

运行：`cargo test --locked -j 8 detail_hero` 和 `cargo test --locked -j 8 view_state`

预期：通过。此时 `DetailHero` 还没有调用方，下一步接上后再跑 clippy。

- [ ] **步骤 3：重写三个详情页**

`src/app/shell/content.rs`：`self.render_album_detail(cx)` 改成 `self.render_album_detail(window, cx)`，`self.render_playlist_detail(cx)` 改成 `self.render_playlist_detail(window, cx)`。

`src/app/library/detail_view.rs` 顶部加：

```rust
use crate::app::widgets::detail_hero::DetailHero;
use crate::app::widgets::shelf::shelf_section;
```

`render_artist_detail` 整个替换为：

```rust
    pub(in crate::app) fn render_artist_detail(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(artist) = self.state.artist_details.artist() else {
            return div().child(t!("Artist not found")).into_any_element();
        };
        let popular_count = self.state.artist_details.top_songs().len();
        let popular_disabled = self.state.artist_details.top_songs_loading() || popular_count == 0;
        let artist_for_play_all = artist.clone();
        let artist_radio_loading = self.artist_radio_loading(&artist.id);
        let padding = self.shell_layout(window).page_padding;
        let hero = self.render_detail_hero(
            DetailHero {
                kind: t!("Artist").into(),
                title: artist.name.clone().into(),
                cover_id: artist.cover_art.clone(),
                round_cover: true,
                meta: div()
                    .child(t!(
                        "%{albums} albums · %{songs} songs",
                        albums = self.state.artist_details.albums().len(),
                        songs = popular_count
                    ))
                    .into_any_element(),
                actions: h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(
                        round_play_button(
                            "play-artist-all",
                            PlayerIcon::Play,
                            t!("Play all"),
                            tokens::DETAIL_PLAY_BUTTON,
                            cx,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.play_artist(artist_for_play_all.clone());
                            cx.notify();
                        })),
                    )
                    .child(
                        capsule_button("play-artist-popular", cx)
                            .label(t!("Play popular"))
                            .disabled(popular_disabled)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.play_song_at_scope(SongScope::ArtistTop, 0);
                                cx.notify();
                            })),
                    )
                    .child(
                        capsule_button("start-artist-radio", cx)
                            .label(if artist_radio_loading {
                                t!("Building artist radio…")
                            } else {
                                t!("Start artist radio")
                            })
                            .disabled(popular_disabled || artist_radio_loading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.start_artist_radio();
                                cx.notify();
                            })),
                    )
                    .child(self.favorite_button(FavoriteKind::Artist, &artist.id, cx))
                    .into_any_element(),
            },
            window,
            cx,
        );
        let popular: gpui_kit::AnyElement = if self.state.artist_details.top_songs_loading() {
            h_flex()
                .h(px(96.0))
                .w_full()
                .items_center()
                .justify_center()
                .text_size(px(tokens::BODY))
                .text_color(palette(cx).text_secondary)
                .child(t!("Loading popular songs…"))
                .into_any_element()
        } else if let Some(error) = self.state.artist_details.top_songs_error() {
            h_flex()
                .w_full()
                .gap_3()
                .items_center()
                .rounded(px(tokens::radius::COVER))
                .bg(palette(cx).sunken)
                .p_3()
                .child(
                    div()
                        .flex_1()
                        .text_size(px(tokens::BODY))
                        .text_color(palette(cx).text_secondary)
                        .child(t!("Popular songs are unavailable: %{error}", error = error)),
                )
                .child(
                    Button::new("retry-artist-popular")
                        .icon(AppIcon::Refresh)
                        .label(t!("Retry"))
                        .small()
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.load_artist_top_songs();
                            cx.notify();
                        })),
                )
                .into_any_element()
        } else if self.state.artist_details.top_songs().is_empty() {
            div()
                .py_4()
                .text_size(px(tokens::BODY))
                .text_color(palette(cx).text_secondary)
                .child(t!("No popular songs were returned for this artist."))
                .into_any_element()
        } else {
            self.render_song_list(SongScope::ArtistTop, cx)
        };
        let albums = if self.state.artist_details.albums_loading() {
            self.render_album_skeleton_grid(window, cx)
        } else {
            self.render_album_grid(
                "artist-detail",
                self.state.artist_details.albums(),
                cx,
                window,
            )
        };
        v_flex()
            .child(hero)
            .child(
                v_flex()
                    .px(px(padding))
                    .pb(px(padding))
                    .gap(px(tokens::SECTION_GAP))
                    .child(shelf_section(
                        section_title(t!("Popular songs"), cx).into_any_element(),
                        popular,
                    ))
                    .child(shelf_section(
                        section_title(t!("Albums"), cx).into_any_element(),
                        v_flex()
                            .gap_3()
                            .when(self.state.artist_details.albums_error().is_some(), |this| {
                                this.child(self.render_retry_row(
                                    "retry-artist-albums",
                                    cx,
                                    |this, cx| {
                                        this.load_artist_albums();
                                        cx.notify();
                                    },
                                ))
                            })
                            .child(albums),
                    )),
            )
            .into_any_element()
    }
```

`render_album_detail` 整个替换为：

```rust
    pub(in crate::app) fn render_album_detail(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(album) = self.state.album_details.album() else {
            return div().child(t!("Album not found")).into_any_element();
        };
        let actions_disabled =
            self.state.album_details.loading() || self.state.album_details.songs().is_empty();
        let padding = self.shell_layout(window).page_padding;
        let hero = self.render_detail_hero(
            DetailHero {
                kind: t!("Album").into(),
                title: album.name.clone().into(),
                cover_id: album.cover_art.clone(),
                round_cover: false,
                meta: h_flex()
                    .gap_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_color(palette(cx).brand)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.artist_name_link(
                                "album-detail-artist",
                                album.artist.clone(),
                                album.artist_id.clone(),
                                album.cover_art.clone(),
                                cx,
                            )),
                    )
                    .when_some(album.year, |this, year| {
                        this.child(t!("- %{year}", year = year))
                    })
                    .into_any_element(),
                actions: h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(
                        round_play_button(
                            "play-album",
                            PlayerIcon::Play,
                            t!("Play album"),
                            tokens::DETAIL_PLAY_BUTTON,
                            cx,
                        )
                        .disabled(actions_disabled)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.play_song_at_scope(SongScope::AlbumDetail, 0);
                            cx.notify();
                        })),
                    )
                    .child(
                        capsule_button("append-album", cx)
                            .icon(AppIcon::Queue)
                            .label(t!("Append all"))
                            .disabled(actions_disabled)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.append_songs_at_scope(SongScope::AlbumDetail);
                                cx.notify();
                            })),
                    )
                    .child(
                        capsule_button("album-add-to-playlist", cx)
                            .label(t!("Add to playlist…"))
                            .disabled(self.state.playlists.editing() || actions_disabled)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.choose_playlist(
                                    crate::playlist::song_ids(this.state.album_details.songs()),
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .into_any_element(),
            },
            window,
            cx,
        );
        v_flex()
            .h_full()
            .min_h_0()
            .child(hero)
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .px(px(padding))
                    .pb(px(padding))
                    .gap_3()
                    .when(self.state.album_details.failed(), |this| {
                        this.child(self.render_retry_row(
                            "retry-album-details",
                            cx,
                            |this, cx| {
                                this.load_album_details();
                                cx.notify();
                            },
                        ))
                    })
                    .child(if self.state.album_details.loading() {
                        self.render_list_skeleton(6, cx)
                    } else {
                        self.render_song_list(SongScope::AlbumDetail, cx)
                    }),
            )
            .into_any_element()
    }
```

`render_playlist_detail` 整个替换为：

```rust
    pub(in crate::app) fn render_playlist_detail(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(playlist) = self.state.playlists.playlist() else {
            return div().child(t!("Playlist not found")).into_any_element();
        };
        let rename = playlist.clone();
        let delete = playlist.clone();
        let read_only = !crate::playlist::can_edit(playlist, &self.config.username);
        let edit_disabled =
            read_only || self.state.playlists.editing() || self.state.playlists.detail_loading();
        let playback_disabled =
            self.state.playlists.songs().is_empty() || self.state.playlists.detail_loading();
        let padding = self.shell_layout(window).page_padding;
        let hero = self.render_detail_hero(
            DetailHero {
                kind: t!("Playlist").into(),
                title: playlist.name.clone().into(),
                cover_id: playlist.cover_art.clone(),
                round_cover: false,
                meta: div()
                    .child(t!(
                        "%{count} tracks",
                        count = self.state.playlists.songs().len()
                    ))
                    .into_any_element(),
                actions: h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(
                        round_play_button(
                            "play-playlist",
                            PlayerIcon::Play,
                            t!("Play all"),
                            tokens::DETAIL_PLAY_BUTTON,
                            cx,
                        )
                        .disabled(playback_disabled)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.play_song_at_scope(SongScope::PlaylistDetail, 0);
                            cx.notify();
                        })),
                    )
                    .child(
                        capsule_button("append-playlist", cx)
                            .icon(AppIcon::Queue)
                            .label(t!("Append all"))
                            .disabled(playback_disabled)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.append_songs_at_scope(SongScope::PlaylistDetail);
                                cx.notify();
                            })),
                    )
                    .child(
                        capsule_button("rename-playlist", cx)
                            .label(t!("Rename"))
                            .disabled(edit_disabled)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.name_playlist(Some(rename.clone()), vec![], window, cx)
                            })),
                    )
                    .child(
                        capsule_button("delete-playlist", cx)
                            .label(t!("Delete"))
                            .disabled(edit_disabled)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.delete_playlist(delete.clone(), window, cx)
                            })),
                    )
                    .child(
                        Button::new("refresh-playlist")
                            .icon(AppIcon::Refresh)
                            .tooltip(t!("Refresh playlist"))
                            .small()
                            .ghost()
                            .disabled(
                                self.state.playlists.editing()
                                    || self.state.playlists.detail_loading(),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.load_playlist_detail();
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
            },
            window,
            cx,
        );
        v_flex()
            .h_full()
            .min_h_0()
            .child(hero)
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .px(px(padding))
                    .pb(px(padding))
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(tokens::META))
                            .text_color(palette(cx).text_muted)
                            .child(if self.state.playlists.editing() {
                                t!("Saving playlist…")
                            } else if self.state.playlists.detail_loading() {
                                t!("Loading playlist…")
                            } else if read_only {
                                t!("Shared playlist · Only the owner can edit it.")
                            } else if self.state.playlists.songs().is_empty() {
                                t!("This playlist is empty. Use a song’s actions menu to add tracks.")
                            } else {
                                t!("Use a song’s actions menu to move or remove it.")
                            }),
                    )
                    .child(self.render_song_list(SongScope::PlaylistDetail, cx)),
            )
            .into_any_element()
    }
```

三个语言文件加 `"Playlist"`（en: `Playlist`，zh-CN: `播放列表`，zh-TW: `播放清單`）。

- [ ] **步骤 4：改搜索页**

`src/app/search/view.rs`：

顶部加 `use crate::app::widgets::shelf::shelf_section;`。

把 `.gap_5()` 改成 `.gap(px(tokens::SECTION_GAP))`，把页头和搜索框这两个 `.child` 替换为：

```rust
            .child(
                v_flex()
                    .gap_6()
                    .child(self.page_header(
                        t!("Search"),
                        t!("Find music across the entire server library."),
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                h_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .h(px(44.0))
                                    .px_4()
                                    .items_center()
                                    .rounded_full()
                                    .bg(palette(cx).sunken)
                                    .child(Input::new(&self.search_input).appearance(false).w_full()),
                            )
                            .child(
                                Button::new("run-search")
                                    .label(t!("Search"))
                                    .custom(brand_button_style(cx))
                                    .rounded(px(tokens::radius::FULL))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.submit_search(cx);
                                        cx.notify();
                                    })),
                            )
                            .child(self.header_refresh_button(
                                "refresh-search",
                                t!("Run search again"),
                                self.state.search.loading(),
                                cx,
                                |this, cx| this.submit_search(cx),
                            )),
                    ),
            );
```

把三段结果里“标题加内容”分别替换为 `shelf_section`，例如艺术家：

```rust
                if !results.artists.is_empty() {
                    content = content.child(shelf_section(
                        section_title(t!("Artists"), cx).into_any_element(),
                        self.render_artist_grid(&results.artists, cx, window),
                    ));
                } else if filter == SearchFilter::Artists {
```

专辑段用 `self.render_album_grid("search", &results.albums, cx, window)` 和 `t!("Albums")`，歌曲段用 `self.render_song_list(SongScope::Search, cx)` 和 `t!("Songs")`，写法相同。空结果提示和过滤按钮不变。

- [ ] **步骤 5：运行标准验证**

预期：全部通过。

- [ ] **步骤 6：请用户查看**

告诉用户：打开专辑、艺术家、播放列表详情页和搜索页。检查头部封面色渐隐、圆形主按钮、胶囊次要按钮齐全；把窗口缩到 900×600，确认专辑页歌曲列表还有足够高度；再打开一张从未播放过的专辑，确认头部在封面加载后才变色、之前不出错。

---

### Task 7: 歌曲列表

**Files:**
- Modify: `src/app/shell/view_helpers.rs`、`src/app/songs/scope.rs`、`src/app/songs/scope/tests.rs`、`src/app/songs/row_view.rs`、`src/app/songs/view.rs`

**Interfaces:**
- Consumes: `palette`、`raised_surface`、`tokens::radius::ROW`。
- Produces: `SongScope::shows_row_cover(self) -> bool`；`SONG_ROW_HEIGHT == 56.0`。

- [ ] **步骤 1：写失败测试**

`src/app/songs/scope/tests.rs` 末尾加：

```rust
#[test]
fn only_album_detail_hides_the_repeated_row_cover() {
    assert!(!SongScope::AlbumDetail.shows_row_cover());
    for scope in [
        SongScope::Queue,
        SongScope::History,
        SongScope::PlaylistDetail,
        SongScope::Favorites,
        SongScope::Search,
        SongScope::ArtistTop,
    ] {
        assert!(scope.shows_row_cover(), "{scope}");
    }
}
```

运行：`cargo test --locked -j 8 songs::scope`

预期：编译失败，`shows_row_cover` 未定义。

- [ ] **步骤 2：实现并改行高**

`src/app/songs/scope.rs` 的 `fills_viewport` 之后加：

```rust
    /// Every row on an album page has the same cover, so showing it repeats noise.
    pub(in crate::app) fn shows_row_cover(self) -> bool {
        self != Self::AlbumDetail
    }
```

`src/app/shell/view_helpers.rs` 第 4、5 行替换为：

```rust
/// 歌曲行高。uniform_list 限高模式和封面预取都依赖它。
pub(in crate::app) const SONG_ROW_HEIGHT: f32 = 56.0;
```

运行：`cargo test --locked -j 8 songs::`

预期：通过。如果有测试写死了 61，把它改成使用 `SONG_ROW_HEIGHT`。

- [ ] **步骤 3：改歌曲行**

`src/app/songs/row_view.rs`：

删除顶部的 `use crate::app::covers::artwork::accent_foreground;`，并在 `view_helpers` 的导入里加上 `SONG_ROW_HEIGHT`。

把从 `let row_accent = self.now_playing_accent(cx);` 到第一个 `let album = ...` 之前的两行替换为：

```rust
        let p = palette(cx);
```

把从 `// 普通悬浮只使用主题的中性列表表面` 到 `let row = if multi_selected {...};` 结束这一段替换为：

```rust
        // 高亮优先级：多选 > 列表光标 > 正在播放，三者都用品牌淡底；
        // 已带状态的行悬浮时不再变色，文字对比度始终有保证。
        let highlighted = multi_selected || keyboard_selected || current;
        // uniform_list 把每行当作独立根元素布局（Definite 可用宽度），
        // 不会像普通容器那样横向拉伸，必须显式 w_full：否则行宽收缩为
        // 内容宽度，行尾按钮、悬停/正在播放高亮都无法纵向对齐。
        let row = h_flex()
            .id(SharedString::from(key.clone()))
            .w_full()
            .h(px(SONG_ROW_HEIGHT))
            .px_3()
            .gap(if queue_drawer { px(8.0) } else { px(12.0) })
            .relative()
            .rounded(px(tokens::radius::ROW))
            .when(highlighted, |this| this.bg(p.brand_soft))
            .when(!highlighted, |this| this.hover(|style| style.bg(p.sunken)))
            .cursor_pointer();
```

把 `position_indicator` 里的颜色替换：`.rounded_md().bg(row_accent)` 改成 `.rounded(px(tokens::radius::CONTROL)).bg(p.brand)`；`.text_color(row_accent_foreground)` 改成 `.text_color(p.on_brand)`；数字的 `.text_color(if keyboard_selected || current { row_accent } else { cx.theme().muted_foreground })` 改成 `.text_color(if keyboard_selected || current { p.brand } else { p.text_muted })`，`.text_xs()` 改成 `.text_size(px(tokens::CAPTION))`。

把固定 42 宽的封面块替换为（专辑页不显示）：

```rust
        .when(scope.shows_row_cover(), |row| {
            row.child(
                div()
                    .w(px(42.0))
                    .h(px(42.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(self.render_cover_styled(
                        song.cover_art.as_deref(),
                        40.0,
                        tokens::radius::CONTROL,
                        Vec::new(),
                    )),
            )
        })
```

其余替换（同一个文件内）：
- 歌名块的 `.text_color(if current { row_accent } else { cx.theme().foreground })` 改成 `.text_color(if current { p.brand } else { p.text })`。
- 歌名 `.text_sm()` 改成 `.text_size(px(tokens::BODY))`。
- 来源标签 `.bg(row_accent.opacity(0.1))` 改成 `.bg(p.brand_soft)`，`.text_xs().text_color(row_accent)` 改成 `.text_size(px(tokens::CAPTION)).text_color(p.brand)`。
- 副标题 `.text_xs().text_color(cx.theme().muted_foreground)` 改成 `.text_size(px(tokens::META)).text_color(p.text_secondary)`。
- 艺术家列和时长列的 `.text_sm().text_color(cx.theme().muted_foreground)` 改成 `.text_size(px(tokens::META)).text_color(p.text_secondary)`。
- 播放按钮的 `.text_color(row_accent)` 改成 `.text_color(p.brand)`。
- 移除按钮的 `.text_color(cx.theme().muted_foreground)` 改成 `.text_color(p.text_secondary)`。

`render_playing_indicator`：`.bg(cx.theme().info)` 改成 `.bg(palette(cx).brand)`，`.text_color(cx.theme().muted_foreground)` 改成 `.text_color(palette(cx).text_muted)`。

- [ ] **步骤 4：改列头和多选操作条**

`src/app/songs/view.rs`：

把列头 `h_flex()` 的这几行：

```rust
                        .h(px(36.0))
                        .flex_none()
                        .px_3()
                        .gap_3()
                        .border_b_1()
                        .border_color(cx.theme().border.opacity(0.72))
                        .bg(cx.theme().secondary.opacity(0.14))
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(cx.theme().muted_foreground)
                        .child(div().w(px(28.0)))
                        .child(div().w(px(42.0)))
```

替换为：

```rust
                        .h(px(32.0))
                        .flex_none()
                        .px_3()
                        .gap_3()
                        .items_center()
                        .text_size(px(tokens::CAPTION))
                        .text_color(palette(cx).text_muted)
                        .child(div().w(px(28.0)))
                        .when(scope.shows_row_cover(), |this| this.child(div().w(px(42.0))))
```

`let cap = ...` 里的 `px(36.0)` 改成 `px(32.0)`。

`render_multi_action_bar` 开头的 `h_flex()` 到 `.border_color(cx.theme().border)` 替换为：

```rust
        raised_surface(
            h_flex()
                .h(px(44.0))
                .flex_none()
                .m_2()
                .px_3()
                .gap_2()
                .items_center(),
            tokens::radius::LARGE,
            cx,
        )
```

并把 `.text_sm().font_weight(FontWeight::SEMIBOLD).text_color(cx.theme().primary)` 改成 `.text_size(px(tokens::BODY)).font_weight(FontWeight::SEMIBOLD).text_color(palette(cx).brand)`。

- [ ] **步骤 5：运行标准验证**

预期：全部通过。

- [ ] **步骤 6：请用户查看**

告诉用户：看专辑页（没有行封面）、收藏和队列（有小封面）、当前播放行的淡玫瑰底和跳动音柱、Ctrl/Shift 多选和浮起的操作条；单击选中、双击播放、右键菜单、键盘上下都要试一遍；长列表滚动时封面照常加载。

---

### Task 8: 正在播放页与迷你播放器

**Files:**
- Create: `src/app/player/now_playing/tests.rs`
- Modify: `src/app/shell/render.rs`、`src/app/player/now_playing.rs`、`src/app/player/mini_view.rs`、`src/app/player/controls.rs`、`src/app/covers/view.rs`、`src/app/style/tokens.rs`

**Interfaces:**
- Consumes: `cover_tint`、`tint_background`、`TintSurface`、`capsule_button`、`brand_button_style`。
- Produces: `NavidromeApp::now_playing_cover_palette(&self) -> Option<(Hsla, Hsla)>`。删除 `now_playing_palette`、`now_playing_accent`、`play_button_style`。

- [ ] **步骤 1：写歌词不透明度的失败测试**

`src/app/player/now_playing/tests.rs`：

```rust
use super::*;

#[test]
fn opacity_falls_with_distance_and_never_below_the_floor() {
    let values = [1, 2, 3, 4, 10, usize::MAX].map(lyric_opacity);
    assert!(values.windows(2).all(|pair| pair[0] >= pair[1]));
    assert_eq!(lyric_opacity(usize::MAX), 0.55);
    assert!(values.iter().all(|value| *value >= 0.55));
}
```

- [ ] **步骤 2：改正在播放页**

`src/app/style/tokens.rs` 加：

```rust
pub(in crate::app) const NOW_PLAYING_TITLE: f32 = 30.0;
pub(in crate::app) const LYRIC_ACTIVE: f32 = 28.0;
pub(in crate::app) const LYRIC: f32 = 20.0;
```

`src/app/player/now_playing.rs`：

删除 `let accent = self.now_playing_accent(cx);`，在原位加：

```rust
        let p = palette(cx);
        let serif = crate::typography::serif_font_family(cx);
```

把无歌曲时标题和说明里的 `.text_color(cx.theme().muted_foreground)` 都改成 `.text_color(palette(cx).text_secondary)`。其余 `cx.theme().muted_foreground` 也都改成 `p.text_secondary`。

把歌词行末尾的高亮分支替换为：

```rust
                    let line = line.font_family(serif.clone());
                    if current {
                        line.text_size(px(tokens::LYRIC_ACTIVE))
                            .font_weight(FontWeight::BOLD)
                            .text_color(p.brand)
                            .with_animation(
                                SharedString::from(format!("active-lyric-{}-{index}", song.id)),
                                Animation::new(Duration::from_millis(220))
                                    .with_easing(ease_out_quint()),
                                |this, delta| this.opacity(0.64 + delta * 0.36),
                            )
                            .into_any_element()
                    } else if synced {
                        line.text_size(px(tokens::LYRIC))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.text.opacity(lyric_opacity(distance)))
                            .into_any_element()
                    } else {
                        line.text_size(px(tokens::LYRIC))
                            .text_color(p.text.opacity(0.72))
                            .into_any_element()
                    }
```

歌词自动滚动用的是每行的实际布局位置（`bounds_for_item`），不依赖字号，不需要改。

把歌名和按钮替换：

```rust
                            .child(serif_heading(song.title.clone(), tokens::NOW_PLAYING_TITLE, cx))
```

替换原来 `div().text_xl().font_weight(FontWeight::SEMIBOLD).child(song.title.clone())`。艺术家那个 `div().text_sm().text_color(...)` 改成 `.text_size(px(tokens::BODY)).text_color(p.brand).font_weight(FontWeight::SEMIBOLD)`；专辑 `.text_xs()` 改成 `.text_size(px(tokens::META))`。

“开始歌曲电台”按钮的 `Button::new("now-playing-song-radio")` 改成 `capsule_button("now-playing-song-radio", cx)`，并删除 `.outline()`。

文件末尾（`impl` 之后）加：

```rust
fn lyric_opacity(distance: usize) -> f32 {
    match distance {
        1 => 0.78,
        2 => 0.68,
        3 => 0.6,
        _ => 0.55,
    }
}

#[cfg(test)]
mod tests;
```

运行：`cargo test --locked -j 8 player::now_playing`

预期：通过。

- [ ] **步骤 3：改取色入口和窗口背景**

`src/app/player/controls.rs`：删除 `now_playing_palette`、`now_playing_accent`、`play_button_style`，删除顶部的 `use crate::app::covers::artwork::{...};`，加入：

```rust
    pub(in crate::app) fn now_playing_cover_palette(&self) -> Option<(Hsla, Hsla)> {
        self.state
            .now_playing
            .as_ref()
            .and_then(|song| song.cover_art.as_deref())
            .and_then(|cover_id| self.state.covers.palette(cover_id))
    }
```

`src/app/covers/view.rs`：把 `let metal_tint = self.now_playing_accent(cx);` 改成：

```rust
        let metal_tint = self
            .now_playing_cover_palette()
            .map_or(palette(cx).text_muted, |(_, accent)| accent);
```

`src/app/shell/render.rs`：顶部加 `use crate::app::style::cover_tint::{cover_tint, tint_background, TintSurface};`。把 `if self.state.view == View::NowPlaying { ... }` 整段替换为：

```rust
        if self.state.view == View::NowPlaying {
            let tint = cover_tint(
                TintSurface::NowPlaying,
                self.now_playing_cover_palette(),
                cx.theme().background,
                cx.theme().is_dark(),
            );

            return v_flex()
                .size_full()
                .text_color(cx.theme().foreground)
                .bg(tint_background(tint, cx.theme().background, 120.0))
                .child(
                    TitleBar::new()
                        .on_close_window(Self::close_button_handler(cx))
                        .bg(cx.theme().transparent)
                        .border_color(cx.theme().transparent)
                        .child(self.render_now_playing_titlebar(cx)),
                )
                .child(
                    div()
                        .relative()
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .child(self.render_now_playing(window, cx))
                        .when(self.state.queue_drawer_open, |this| {
                            this.child(self.render_queue_drawer(
                                layout.queue_drawer_width,
                                true,
                                cx,
                            ))
                        })
                        .when(settings_open, |this| {
                            this.child(self.render_settings_overlay(layout.auto_collapsed, cx))
                        }),
                )
                .child(self.render_player(window, cx))
                .into_any_element();
        }
```

`src/app/shell/titlebar.rs` 的 `render_now_playing_titlebar`：`.text_xs().font_weight(FontWeight::MEDIUM).text_color(cx.theme().muted_foreground)` 改成 `.text_size(px(tokens::CAPTION)).text_color(palette(cx).text_secondary)`。

- [ ] **步骤 4：改迷你播放器**

`src/app/player/mini_view.rs`：

删除 `use super::controls::play_button_style;`，顶部加 `use crate::app::style::cover_tint::{cover_tint, tint_background, TintSurface};`。

把这一段：

```rust
        let (base, raw_accent) = self.now_playing_palette(cx);
        let accent = self.now_playing_accent(cx);
        let light = theme.background.l >= 0.5;
        let base_strength = if light { 0.36 } else { 0.3 };
        let accent_strength = if light { 0.28 } else { 0.24 };
        let bg_start = theme.background.blend(base.opacity(base_strength));
        let bg_end = theme.background.blend(raw_accent.opacity(accent_strength));
        let bg_animation = Animation::new(Duration::from_secs(20)).repeat();
```

替换为：

```rust
        let accent = palette(cx).brand;
        let background = tint_background(
            cover_tint(
                TintSurface::MiniPlayer,
                self.now_playing_cover_palette(),
                theme.background,
                theme.is_dark(),
            ),
            theme.background,
            120.0,
        );
```

封面阴影里的 `hsla(0.0, 0.0, 0.0, 0.28)` 那个 `.when(!is_playing, ...)` 替换为：

```rust
            .when(!is_playing, |this| this.shadow(cover_shadow(false, cx)))
```

`.custom(play_button_style(accent, cx))` 改成 `.custom(brand_button_style(cx))`。

把根元素 `.with_animation("mini-player-background", bg_animation, move |this, delta| {...})` 整段删除，并在 `.relative().size_full()` 后加 `.bg(background)`。

- [ ] **步骤 5：删除失去调用方的取色函数**

此时 `accent_foreground` 和 `adjust_lightness` 已没有调用方。从 `src/app/covers/artwork.rs` 删除这两个函数，并从 `src/app/covers/artwork/tests.rs` 删除测试 `accent_button_foreground_prefers_the_stronger_contrast`。`readable_accent` 还被 `src/app/import/view.rs` 使用，留到 Task 9。

- [ ] **步骤 6：运行标准验证**

预期：全部通过。

- [ ] **步骤 7：请用户查看**

告诉用户：看正在播放页（暖底封面渐变不再转动、衬线歌词、黑胶和唱臂正常、歌词点击跳转和自动滚动正常）和迷你播放器（静止背景、玫瑰红播放钮、滚轮调音量反馈）。分别用彩色封面、灰色封面和无封面歌曲看一次。

---

### Task 9: 设置、弹窗与清理

**Files:**
- Modify: `src/app/settings/overlay.rs`、`src/app/settings/navigation_view.rs`、`src/app/settings/view.rs`、`src/app/settings/playback_view.rs`、`src/app/settings/desktop_view.rs`、`src/app/settings/appearance_view.rs`、`src/app/server/settings_view.rs`、`src/app/server/list_view.rs`、`src/app/shortcuts/row_view.rs`、`src/app/shortcuts/settings_view.rs`、`src/app/import/view.rs`、`src/app/import/orchestration.rs`、`src/app/playlists/dialog.rs`、`src/app/songs/info_view.rs`、`src/app/server/remove_dialog.rs`、`src/app/style/surface.rs`、`src/app/style/tokens.rs`、`src/app/covers/artwork.rs`、`src/app/covers/artwork/tests.rs`、`src/app/prelude.rs`

**Interfaces:**
- Consumes: `section_title`、`raised_surface`、`palette`。
- Produces: `surface::raised_dialog(dialog: Dialog, cx: &App) -> Dialog`、`tokens::SCRIM`。

- [ ] **步骤 1：改设置覆盖层**

`src/app/style/tokens.rs` 加：

```rust
/// Fixed black dim: darkens the page behind overlays in both modes.
pub(in crate::app) fn scrim() -> Hsla {
    gpui_kit::hsla(0.0, 0.0, 0.0, 0.35)
}
```

`src/app/settings/overlay.rs`：`.bg(hsla(0.0, 0.0, 0.0, 0.35))` 改成 `.bg(tokens::scrim())`。

把设置卡片的 `v_flex().id("settings-card")...` 开头到 `.overflow_hidden()` 替换为：

```rust
                raised_surface(
                    v_flex()
                        .id("settings-card")
                        .w(px(960.0))
                        .h(px(650.0))
                        .max_w(relative(if compact { 0.96 } else { 0.94 }))
                        .max_h(relative(if compact { 0.94 } else { 0.88 }))
                        .mt(px(if compact { 16.0 } else { 48.0 }))
                        .overflow_hidden(),
                    tokens::radius::LARGE,
                    cx,
                )
```

标题行：删除 `.border_b_1()` 和 `.border_color(cx.theme().border)`，把 `div().text_sm().font_weight(FontWeight::SEMIBOLD).child(t!("Settings"))` 换成 `section_title(t!("Settings"), cx)`。`.when(compact, |this| this.px_5().py_4()).when(!compact, |this| this.px_6().py_5())` 改成 `.when(compact, |this| this.px_4().py_4()).when(!compact, |this| this.px_6().py_6())`。

`src/app/settings/navigation_view.rs` 的 `render_settings_navigation`：两个分支都删除 `.border_b_1()` / `.border_r_1()`、`.border_color(cx.theme().border)`、`.bg(cx.theme().secondary.opacity(0.25))`。`settings_navigation_button` 的 `.ghost().selected(selected)` 改成 `.custom(soft_toggle_style(selected, cx)).rounded(px(tokens::radius::CONTROL))`。

`src/app/settings/view.rs`：`.gap_5()` 改成 `.gap_6()`，把 `div().text_lg().font_weight(FontWeight::SEMIBOLD).child(presentation.title)` 换成 `section_title(presentation.title, cx)`，说明文字 `.text_sm().text_color(cx.theme().muted_foreground)` 改成 `.text_size(px(tokens::META)).text_color(palette(cx).text_secondary)`。

- [ ] **步骤 2：去掉设置分组的边框面板**

写脚本 `/tmp/settings_groups.py` 并运行 `python3 /tmp/settings_groups.py`（脚本只改下面三个文件里列出的七个分组）：

```python
#!/usr/bin/env python3
import re

ROOT = "/home/hn/code/rust/naviclient/src/app/settings/"
TITLES = {
    "playback_view.rs": ["Streaming & cache", "Audio output", "Volume normalization", "Lyrics"],
    "desktop_view.rs": ["System integration"],
    "appearance_view.rs": ["Theme", "Language"],
}
PANEL = re.compile(
    r"\s*\.p_4\(\)\s*\.border_t_1\(\)\s*\.border_b_1\(\)\s*"
    r"\.border_color\(cx\.theme\(\)\.border\.opacity\(0\.72\)\)"
)
for name, titles in TITLES.items():
    path = ROOT + name
    text = open(path, encoding="utf-8").read()
    text, panels = PANEL.subn("", text)
    assert panels == len(titles), (name, panels)
    for title in titles:
        heading = re.compile(
            r"\.child\(\s*div\(\)\s*\.text_sm\(\)\s*\.font_weight\(FontWeight::MEDIUM\)\s*"
            r"\.child\(t!\(\"" + re.escape(title) + r"\"\)\),\s*\)"
        )
        text, count = heading.subn(
            '.child(section_title(t!("' + title + '"), cx))', text
        )
        assert count == 1, (name, title, count)
    open(path, "w", encoding="utf-8").write(text)
```

预期：无输出、无断言失败。然后运行 `cargo fmt`。

- [ ] **步骤 3：去掉服务器和快捷键的边框**

`src/app/server/settings_view.rs`：把

```rust
        let mut card = v_flex()
            .w_full()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border.opacity(0.72))
            .bg(cx.theme().secondary.opacity(0.18))
```

替换为：

```rust
        let mut card = v_flex()
            .w_full()
            .gap_6()
```

内部的 `.p_4()` 删除；把表单前的分隔线 `.child(div().h(px(1.0)).bg(cx.theme().border))` 删除（间距由 `.gap_6()` 提供）。

`src/app/server/list_view.rs`：删除 `.border_1()` 和整个 `.border_color(if is_active {...} else {...})` 及其注释；把 `.bg(if is_active { cx.theme().list_active } else { cx.theme().background })` 改成 `.when(is_active, |this| this.bg(palette(cx).brand_soft))`；把 hover 的 `.hover(|style| style.bg(cx.theme().list_active))` 改成 `.hover(|style| style.bg(palette(cx).sunken))`；`.rounded(px(8.0))` 改成 `.rounded(px(tokens::radius::CONTROL))`，`.rounded(px(999.0))` 改成 `.rounded(px(tokens::radius::FULL))`。

`src/app/shortcuts/row_view.rs`：`render_shortcut_rows` 里删除 `.rounded_lg()`、`.border_1()`、`.border_color(cx.theme().border.opacity(0.72))`、`.overflow_hidden()`，并改 `.children(...)` 的闭包，第四个参数不再需要：

```rust
            .children(
                actions
                    .iter()
                    .map(|action| self.render_shortcut_row(*action, global, cx)),
            )
```

`render_shortcut_row` 删除 `separated: bool` 参数和 `.when(separated, |row| {...})` 块，`.px_4().py_3()` 改成 `.py_3()`。

`src/app/shortcuts/settings_view.rs`：`.pt_5().border_t_1().border_color(cx.theme().border)` 改成 `.pt_6()`；把“App shortcuts”“Global shortcuts”两处 `div().text_sm().font_weight(FontWeight::MEDIUM).child(t!(...))` 换成 `section_title(t!(...), cx)`；`render_shortcut_group` 里的标题保持 `.text_sm().font_weight(FontWeight::MEDIUM)`（它是分组内的小标题）。

- [ ] **步骤 4：统一弹窗**

`src/app/style/surface.rs` 加：

```rust
/// Dialogs are raised surfaces; the kit's radius token already gives 20px.
pub(in crate::app) fn raised_dialog(dialog: Dialog, cx: &App) -> Dialog {
    let p = palette(cx);
    let dialog = dialog.bg(p.raised);
    if cx.theme().is_dark() {
        dialog.border_color(p.border)
    } else {
        dialog.border_color(p.raised)
    }
}
```

文件顶部加 `use gpui_kit::component::dialog::Dialog;`。

四个打开弹窗的地方都包一层（`src/app/playlists/dialog.rs:173`、`src/app/songs/info_view.rs:20`、`src/app/import/orchestration.rs:80`、`src/app/server/remove_dialog.rs:45`），写法相同。歌单：

```rust
                window.open_dialog(cx, move |dialog, _, cx| {
                    render_app
                        .update(cx, |this, cx| {
                            let dialog = this.render_playlist_dialog(dialog, cx);
                            crate::app::style::surface::raised_dialog(dialog, cx)
                        })
                        .expect("app owns dialog")
                });
```

其余三处把 `render_playlist_dialog` 分别换成 `render_song_info_dialog`、`render_import_dialog`、`render_remove_server_dialog`，变量名和 `expect` 文字保持各自原样。

输入框按 spec 用凹陷表面：`src/app/import/view.rs:35`、`src/app/import/view.rs:103`、`src/app/playlists/dialog_view.rs:28`、`src/app/server/form_view.rs:115` 这四处 `Input::new(...)` 后面都加 `.bg(palette(cx).sunken)`。（搜索框在 Task 6 已放进凹陷的胶囊，不用改。）如果某处闭包里取不到 `cx`，在闭包外先取 `let sunken = palette(cx).sunken;`。

`src/app/import/view.rs`：第 3 行 `use crate::app::covers::artwork::readable_accent;` 删除，第 63 行改成 `let bar = palette(cx).brand;`。歌曲行的 `.border_b_1().border_color(cx.theme().border.opacity(0.7))`（第 220 行）删除；滚动列表外框（第 258 行）保留，它和输入框同属“边框留给输入控件”。

- [ ] **步骤 5：清理并检查没有遗留**

`src/app/covers/artwork.rs`：删除 `readable_accent`；给 `relative_luminance` 和 `contrast_ratio` 加 `#[cfg(test)]`（它们现在只有测试在用）。`src/app/covers/artwork/tests.rs` 删除测试 `cover_accent_remains_readable_on_light_and_dark_backgrounds`。

`src/app/style.rs`：删除所有 `#[allow(dead_code)]` 和它们的注释。如果随后 clippy 报某个 token 或字段未使用，先确认是不是有页面漏改；确实用不到的就删掉，不要加回 `allow`。

写脚本 `/tmp/check_literals.sh` 并运行 `bash /tmp/check_literals.sh`：

```bash
#!/bin/bash
cd /home/hn/code/rust/naviclient/src/app
ALLOW='^(covers/view\.rs|covers/artwork\.rs|style/)'
echo "=== hsla literals outside the allowed files (expect none) ==="
grep -rn 'hsla(' --include='*.rs' . | sed 's#^\./##' | grep -vE "$ALLOW" | grep -v '/tests.rs' | grep -v '^tests.rs'
echo "=== radius literals outside style (expect none) ==="
grep -rnE '\.rounded\(px\([0-9]' --include='*.rs' . | sed 's#^\./##' | grep -vE "$ALLOW" | grep -v 'tests.rs'
echo "=== off-scale spacing helpers (expect none) ==="
grep -rnE '\.(gap|p|px|py|pt|pb|pl|pr|m|mx|my|mt|mb|ml|mr)_5\(\)' --include='*.rs' . | sed 's#^\./##' | grep -vE "$ALLOW" | grep -v 'tests.rs'
echo "=== line dividers left (review each) ==="
grep -rnE '\.border_(t|b|l|r)_1\(\)' --include='*.rs' . | sed 's#^\./##' | grep -vE "$ALLOW" | grep -v 'tests.rs'
```

预期：前三组没有输出。第四组如有输出，逐条对照 spec“不用分割线划分区域”：属于分隔线的删除，输入控件的保留。前三组有输出时，把字面量换成 `tokens` 里的对应值（`.gap_5()` 换成 `.gap_6()` 或 `.gap_4()`，圆角换成 `tokens::radius::*`）。

- [ ] **步骤 6：运行标准验证**

预期：全部通过。

- [ ] **步骤 7：请用户查看**

告诉用户：打开设置的每个分区、四种弹窗（新建歌单、导入歌单、删除服务器、歌曲信息）、音量面板、EQ 面板、提示和菜单。检查没有带边框的面板，选项和文案都在。

---

### Task 10: 文档与最终验收

**Files:**
- Modify: `README.md`、`DEVELOPMENT_PLAN.md`

**Interfaces:**
- Consumes: 前面所有任务的结果。

- [ ] **步骤 1：更新 README**

在 `README.md` 模块列表的 `- `src/app.rs` and `src/app/` - GPUI orchestration, views, and isolated feature state` 下一行加：

```markdown
- `src/app/style.rs` and `src/app/style/` - design tokens, kit theme overrides, serif headings, raised surfaces, button recipes, the shared cover column grid, normalized cover tints and genre color blocks; page code reads its colors and visual sizes from here
- `src/app/shell/layout.rs` - sidebar, drawer, padding and content width computed from the current frame's window size
- `src/app/widgets/shelf.rs` and `src/app/widgets/detail_hero.rs` - Home shelves on the shared grid, and the header shared by album, artist and playlist pages
```

第 526 行开始的那段（“Main titlebars and sidebar entries share section titles and icons...”）里，把“Main titlebars and sidebar entries”改成“Sidebar entries and page headings”。功能列表里加一条（放在 `Responsive album and artist grids` 那条后面）：

```markdown
- Bright magazine-style interface: warm off-white surfaces with a warm black dark mode, serif headings, a fixed rose brand color for controls and selection, soft cover-colored backgrounds on content, floating player and queue cards, and shelves whose cover count follows the window width
```

把同一条的 `Responsive album and artist grids with virtualized-ready component foundations` 保留不动。

- [ ] **步骤 2：更新开发计划**

在 `DEVELOPMENT_PLAN.md` 的 `## 当前能力基线` 里、`重构收尾记录（2026-11-05）` 那一段后面加：

```markdown
界面重构记录（2026-11-06）：界面改为明亮杂志风格。新增 `src/app/style/` 集中设计规则，
组件库主题由其覆盖；框架、首页货架、媒体库、详情页、歌曲列表、正在播放、设置与弹窗
均已改造，交互规则不变。封面色铺底前先归一化，由遍历测试保证文字不低于 4.5:1。
设计见 `docs/superpowers/specs/2026-11-06-ui-redesign-design.md`。
```

在 `## 36. 底部播放控制栏视觉与布局优化` 的 `### 实施任务` 前加：

```markdown
2026-11-06 界面重构已完成悬浮卡片、居中布局和无分割线；背景使用不透明表面，半透明
与背景模糊未做（GPUI 没有元素级背景模糊）。下列未勾选项仍待实机核对。
```

- [ ] **步骤 3：最终验证**

运行标准验证。然后运行 Windows 类型检查（本机没有 mingw，用占位工具链；产物放在 `target/` 之外）：

```bash
#!/bin/bash
# 保存为 /tmp/win_check.sh，用 bash 运行
D=/tmp/navi-wincheck; mkdir -p "$D/bin"
printf '#!/bin/sh\nout=\nwhile [ $# -gt 0 ]; do case "$1" in -o) out=$2; shift;; -o*) out=${1#-o};; esac; shift; done\n[ -n "$out" ] && : > "$out"\nexit 0\n' > "$D/fake-cc"
printf '#!/bin/sh\ncase "$2" in *.a) : > "$2";; esac\nexit 0\n' > "$D/fake-ar"
printf '#!/bin/sh\nout=\nwhile [ $# -gt 0 ]; do case "$1" in --output) out=$2; shift;; --output=*) out=${1#--output=};; esac; shift; done\n[ -n "$out" ] && : > "$out"\nexit 0\n' > "$D/bin/x86_64-w64-mingw32-windres"
/bin/chmod +x "$D/fake-cc" "$D/fake-ar" "$D/bin/x86_64-w64-mingw32-windres"
cd /home/hn/code/rust/naviclient
PATH="$D/bin:$PATH" CC_x86_64_pc_windows_gnu="$D/fake-cc" AR_x86_64_pc_windows_gnu="$D/fake-ar" CARGO_TARGET_DIR=/home/hn/.cache/navi-wincheck-target \
  cargo clippy --locked -j 8 --target x86_64-pc-windows-gnu --all-targets -- -D warnings
echo "WIN_EXIT=$?"
rm -rf "$D" /home/hn/.cache/navi-wincheck-target
```

预期：`WIN_EXIT=0`。这只是类型检查和 lint，不是真正的 Windows 构建，最终由 CI 的 `windows-latest` 验证，要在报告里说明。

- [ ] **步骤 4：确认工作区状态**

运行：`git status --short | grep -vE '^( M|\?\?) ' ; git diff --cached --stat | tail -1`

预期：没有已暂存的改动（计划要求不提交、不 `git add`）。

- [ ] **步骤 5：请用户做最终检查并报告**

请用户按 spec 的视觉检查表走一遍：窗口 1280×820、1040 临界宽度、900×600 和全屏；浅色、深色；有封面、无封面、灰色封面；中文、英文界面下的长标题和长翻译。

报告里列出：验证结果和测试数；Windows 只做了类型检查；所有改动都未提交，文件列表用 `git status --short` 给出；用户反馈中未解决的问题。
