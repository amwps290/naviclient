//! Sidebar navigation, collapse control and active-server status.

use crate::app::covers::artwork::{accent_foreground, adjust_lightness};
use crate::app::prelude::*;

impl View {
    /// Titles and icons are shared by the sidebar and main titlebar.
    pub(in crate::app) fn navigation_presentation(self) -> (SharedString, Icon) {
        let (label, icon) = match self {
            Self::Home => (t!("Home"), Icon::new(AppIcon::Home)),
            Self::Favorites => (t!("Favorites"), Icon::new(AppIcon::Heart)),
            Self::Artists | Self::ArtistDetail => (t!("Artists"), Icon::new(AppIcon::Artists)),
            Self::Albums | Self::AlbumDetail => (t!("Albums"), Icon::new(AppIcon::Albums)),
            Self::Genres | Self::GenreDetail => (t!("Genres"), Icon::new(AppIcon::Genres)),
            Self::Playlists | Self::PlaylistDetail => {
                (t!("Playlists"), Icon::new(AppIcon::Playlists))
            }
            Self::Search => (t!("Search"), Icon::new(KitIcon::Search)),
            Self::NowPlaying => (t!("Now Playing"), Icon::new(AppIcon::VinylGrooves)),
        };
        (label.into(), icon)
    }
}

impl NavidromeApp {
    /// 侧边栏底部的折叠/展开按钮；状态持久化到配置。
    pub(in crate::app) fn sidebar_collapse_toggle(
        &self,
        collapsed: bool,
        auto_collapsed: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        if auto_collapsed {
            return div().mt_auto().into_any_element();
        }
        let button = Button::new("sidebar-collapse-toggle")
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
            }));
        if collapsed {
            div()
                .mt_auto()
                .w_full()
                .flex()
                .justify_center()
                .pt_2()
                .child(button)
                .into_any_element()
        } else {
            div()
                .mt_auto()
                .w_full()
                .pt_2()
                .child(button)
                .into_any_element()
        }
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
                        .ghost()
                        .selected(selected)
                        .w_full()
                        .h(px(32.0)),
                )
                .into_any_element()
        } else {
            // 展开态：自定义行（图标 + 标题 + 右端计数徽标）。相比全宽
            // 幽灵按钮，右端的徽标填充了 216px 行尾的大片留白，也顺带让
            // 每个入口自带规模信息（Artists 128、Playlists 6…）。
            // 选中态与设置页服务器列表一致：图标主题色 + 行底色。
            h_flex()
                .id(SharedString::from(format!("nav-{nav_id}")))
                .w_full()
                .h(px(32.0))
                .items_center()
                .gap_2()
                .px_3()
                .rounded(px(8.0))
                .when(selected, |this| this.bg(cx.theme().list_active))
                .when(!selected, |this| {
                    this.cursor_pointer()
                        .hover(|style| style.bg(cx.theme().list_active.opacity(0.55)))
                })
                // accent 是背景 token（亮色 neutral-100、深色 neutral-800），
                // 当图标/文字色用时几乎与底色同值；强调前景统一用 primary。
                .child(icon.with_size(px(16.0)).text_color(if selected {
                    cx.theme().primary
                } else {
                    cx.theme().muted_foreground
                }))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .truncate()
                        .when(selected, |this| this.font_weight(FontWeight::MEDIUM))
                        .child(label),
                )
                .when_some(count.filter(|count| *count > 0), |this, count| {
                    this.child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
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

    /// 侧边栏顶部的服务器胶囊：单行全圆角胶囊（状态圆点 + 当前
    /// 服务器名 + 下拉箭头），点击打开设置（切换/新增/删除都在
    /// 那里）。折叠态只显示图标按钮。
    pub(in crate::app) fn render_sidebar_server(
        &self,
        collapsed: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
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
        let icon = if connected {
            AppIcon::Server
        } else {
            AppIcon::ServerOff
        };

        if collapsed {
            return div()
                .w_full()
                .flex()
                .justify_center()
                .py_1()
                .child(
                    Button::new("sidebar-server")
                        .icon(icon)
                        .tooltip(tooltip)
                        .ghost()
                        .selected(self.state.settings.open)
                        .w_full()
                        .h(px(32.0))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.state.settings.open = true;
                            this.state.settings.section = SettingsSection::Servers;
                            this.refresh_output_devices(cx);
                            cx.notify();
                        })),
                )
                .into_any_element();
        }

        // 已连接：整颗填充当前播放专辑的主色（与“正在播放”页播放按钮
        // 完全同一套配色管线：now_playing_accent 从封面提取、无封面时
        // 回退 theme().info——注意默认主题的 accent token 是灰，直接用
        // 它只会得到灰色胶囊；文字用 accent_foreground 按对比度自动选
        // 黑/白）。连接中/离线是瞬态，保持中性灰底 + 边框。
        let (pill_bg, pill_hover_bg, pill_border, pill_fg, dot_color) = if connected {
            let accent = self.now_playing_accent(cx);
            let fg = accent_foreground(accent);
            (
                accent,
                adjust_lightness(accent, 0.05),
                hsla(0.0, 0.0, 0.0, 0.0),
                fg,
                fg,
            )
        } else if self.state.connection.loading() {
            (
                cx.theme().secondary,
                cx.theme().accent.opacity(0.10),
                cx.theme().border,
                cx.theme().foreground,
                cx.theme().accent.opacity(0.45),
            )
        } else {
            (
                cx.theme().secondary,
                cx.theme().accent.opacity(0.10),
                cx.theme().border,
                cx.theme().foreground,
                cx.theme().border,
            )
        };

        // 胶囊包在整行居中的容器里：名字短时胶囊贴合内容（不像以前那样
        // 撑满整行），长名在整行宽度内截断。
        h_flex()
            .w_full()
            .justify_center()
            .py_1()
            .child(
                h_flex()
                    .id("sidebar-server")
                    .max_w_full()
                    .h(px(30.0))
                    .items_center()
                    .gap_2()
                    .px_4()
                    .rounded_full()
                    .when(!connected, |this| this.border_1().border_color(pill_border))
                    .bg(pill_bg)
                    .cursor_pointer()
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                    .hover(move |style| style.bg(pill_hover_bg))
                    .child(div().flex_none().size(px(6.0)).rounded_full().bg(dot_color))
                    .child(
                        div()
                            .min_w_0()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(pill_fg)
                            .truncate()
                            .child(label),
                    )
                    .child(
                        Icon::new(AppIcon::ChevronDown)
                            .with_size(px(12.0))
                            .text_color(pill_fg.opacity(0.85)),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.settings.open = true;
                        this.state.settings.section = SettingsSection::Servers;
                        this.refresh_output_devices(cx);
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}
