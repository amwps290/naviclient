//! Sidebar navigation, collapse control and active-server status.

use super::layout::ShellLayout;
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
    pub(in crate::app) fn render_sidebar(
        &self,
        layout: &ShellLayout,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let collapsed = layout.sidebar_collapsed;
        if self.browsing_bilibili() {
            return v_flex()
                .w(px(layout.sidebar_width))
                .h_full()
                .p_3()
                .gap_2()
                .child(div().text_sm().child(self.active_server_label()))
                .child(self.nav_button(
                    View::Playlists,
                    Some(self.state.playlists.items().len()),
                    collapsed,
                    cx,
                ))
                .child(self.render_sidebar_footer(collapsed, layout.auto_collapsed, cx))
                .into_any_element();
        }
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
            .child(self.nav_button(View::Artists, Some(self.state.artists.len()), collapsed, cx))
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
            .child(self.nav_button(View::Genres, Some(self.state.genres.len()), collapsed, cx))
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
        let (label, icon) =
            if self.browsing_bilibili() && matches!(view, View::Playlists | View::PlaylistDetail) {
                (
                    t!("My Bilibili favorites").into(),
                    Icon::new(AppIcon::Playlists),
                )
            } else {
                view.navigation_presentation()
            };
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
                        .selected(selected)
                        .rounded(px(tokens::radius::CONTROL))
                        .w_full()
                        .h(px(34.0)),
                )
                .into_any_element()
        } else {
            div()
                .relative()
                .w_full()
                .when(selected, |this| {
                    this.child(
                        div()
                            .absolute()
                            .left_0()
                            .top(px(6.0))
                            .bottom(px(6.0))
                            .w(px(3.0))
                            .bg(p.brand)
                            .rounded_r(px(2.0)),
                    )
                })
                .child(
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
                        })),
                )
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
        let source_count = self.config.sources.len();
        let tooltip = if source_count > 1 {
            t!(
                "%{label} — %{status}\n%{source_count} music sources configured · click to switch",
                label = label,
                status = status,
                source_count = source_count
            )
            .to_string()
        } else {
            t!(
                "%{label} — %{status}\nClick to manage music sources",
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
                        .selected(self.state.settings.open)
                        .rounded(px(tokens::radius::CONTROL))
                        .w_full()
                        .h(px(34.0))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.state.settings.open = true;
                            this.state.settings.section = SettingsSection::Sources;
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
                        this.state.settings.section = SettingsSection::Sources;
                        this.refresh_output_devices(cx);
                        cx.notify();
                    })),
            )
            .child(self.sidebar_collapse_toggle(false, cx))
            .into_any_element()
    }
}
