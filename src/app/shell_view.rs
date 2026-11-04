//! 主窗口标题栏、侧边栏、导航和跨视图播放器样式。

use super::*;

impl NavidromeApp {
    pub(super) fn detail_parent(view: View) -> Option<View> {
        match view {
            View::ArtistDetail => Some(View::Artists),
            View::AlbumDetail => Some(View::Albums),
            View::GenreDetail => Some(View::Genres),
            View::PlaylistDetail => Some(View::Playlists),
            _ => None,
        }
    }

    pub(super) fn titlebar_section_label(&self) -> SharedString {
        match self.state.view {
            View::Home => t!("Home"),
            View::Favorites => t!("Favorites"),
            View::Artists | View::ArtistDetail => t!("Artists"),
            View::Albums | View::AlbumDetail => t!("Albums"),
            View::Genres | View::GenreDetail => t!("Genres"),
            View::Playlists | View::PlaylistDetail => t!("Playlists"),
            View::Search => t!("Search"),
            View::NowPlaying => t!("Now Playing"),
        }
        .into()
    }

    pub(super) fn titlebar_section_icon(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let color = cx.theme().muted_foreground.opacity(0.86);
        match self.state.view {
            View::Home => Icon::new(AppIcon::Home)
                .with_size(px(14.0))
                .text_color(color)
                .into_any_element(),
            View::Favorites => Icon::new(AppIcon::Heart)
                .with_size(px(14.0))
                .text_color(color)
                .into_any_element(),
            View::Artists | View::ArtistDetail => Icon::new(AppIcon::Artists)
                .with_size(px(14.0))
                .text_color(color)
                .into_any_element(),
            View::Albums | View::AlbumDetail => Icon::new(AppIcon::Albums)
                .with_size(px(14.0))
                .text_color(color)
                .into_any_element(),
            View::Genres | View::GenreDetail => Icon::new(AppIcon::Genres)
                .with_size(px(14.0))
                .text_color(color)
                .into_any_element(),
            View::Playlists | View::PlaylistDetail => Icon::new(AppIcon::Playlists)
                .with_size(px(14.0))
                .text_color(color)
                .into_any_element(),
            View::Search => Icon::new(KitIcon::Search)
                .with_size(px(14.0))
                .text_color(color)
                .into_any_element(),
            View::NowPlaying => Icon::new(AppIcon::VinylGrooves)
                .with_size(px(14.0))
                .text_color(color)
                .into_any_element(),
        }
    }

    pub(super) fn render_titlebar_actions(
        &self,
        mini_id: &'static str,
        settings_id: &'static str,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        h_flex()
            .flex_none()
            .items_center()
            .gap_1()
            .occlude()
            .child(
                Button::new(mini_id)
                    .icon(AppIcon::MiniPlayer)
                    .tooltip(t!("Switch to mini player"))
                    .ghost()
                    .small()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.toggle_mini_mode(window, cx);
                    })),
            )
            .child(
                Button::new(settings_id)
                    .icon(AppIcon::Settings)
                    .tooltip(t!("Settings"))
                    .ghost()
                    .small()
                    .selected(self.state.settings_open)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.settings_open = !this.state.settings_open;
                        if this.state.settings_open {
                            this.refresh_output_devices(cx);
                        } else {
                            this.cancel_shortcut_recording();
                        }
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// 主窗口标题栏：左侧呈现页面上下文，右侧保留窗口级工具；
    /// 详情页提供返回其所属媒体库分区的明确入口。
    pub(super) fn render_main_titlebar(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let detail_parent = Self::detail_parent(self.state.view);

        h_flex()
            .h_full()
            .w_full()
            .items_center()
            .pr_2()
            .child(
                h_flex()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap_2()
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .gap_1()
                            .when_some(detail_parent, |this, parent| {
                                this.child(
                                    div().occlude().child(
                                        Button::new("titlebar-back")
                                            .icon(AppIcon::ArrowLeft)
                                            .tooltip(t!("Back to library"))
                                            .ghost()
                                            .small()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.state.settings_open = false;
                                                this.state.view = parent;
                                                cx.notify();
                                            })),
                                    ),
                                )
                            })
                            .child(self.titlebar_section_icon(cx))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(cx.theme().muted_foreground)
                                    .child(self.titlebar_section_label()),
                            ),
                    )
                    .child(div().flex_none().flex().justify_end().child(
                        self.render_titlebar_actions("toggle-mini-player", "title-settings", cx),
                    )),
            )
            .into_any_element()
    }

    pub(super) fn render_now_playing_titlebar(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let track_label = self
            .state
            .now_playing
            .as_ref()
            .map(|song| {
                if song.artist.trim().is_empty() {
                    song.title.clone()
                } else {
                    format!("{} · {}", song.title, song.artist)
                }
            })
            .unwrap_or_else(|| t!("Now Playing").to_string());

        h_flex()
            .h_full()
            .w_full()
            .items_center()
            .pr_2()
            .child(
                h_flex()
                    .w(px(190.0))
                    .flex_none()
                    .items_center()
                    .gap_1()
                    .occlude()
                    .child(
                        Button::new("leave-now-playing")
                            .icon(AppIcon::ArrowLeft)
                            .tooltip(t!("Back to library"))
                            .ghost()
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.leave_now_playing();
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t!("Now Playing")),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .px_4()
                    .text_center()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(cx.theme().muted_foreground)
                    .truncate()
                    .child(track_label),
            )
            .child(self.render_titlebar_actions(
                "now-playing-mini-player",
                "now-playing-settings",
                cx,
            ))
            .into_any_element()
    }

    /// 侧边栏底部的折叠/展开按钮；状态持久化到配置。
    pub(super) fn sidebar_collapse_toggle(
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

    pub(super) fn nav_button(
        &self,
        view: View,
        label: impl Into<SharedString>,
        icon: impl IconNamed,
        count: Option<usize>,
        collapsed: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let label: SharedString = label.into();
        let nav_id = view.nav_id();
        let selected = self.state.view == view
            || matches!(
                (view, self.state.view),
                (View::Artists, View::ArtistDetail)
                    | (View::Albums, View::AlbumDetail)
                    | (View::Genres, View::GenreDetail)
                    | (View::Playlists, View::PlaylistDetail)
            );
        if collapsed {
            // 折叠态：图标按钮居中、拉高加大点击区，标签退化为 tooltip。
            let button = Button::new(SharedString::from(format!("nav-{nav_id}")))
                .icon(icon)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.state.settings_open = false;
                    this.state.view = view;
                    match view {
                        View::Artists if this.state.artists.is_empty() => this.load_artists(),
                        View::Albums if this.state.albums.is_empty() => this.load_albums(),
                        View::Genres if this.state.genres.is_empty() => this.load_genres(),
                        View::Playlists if this.state.playlists.is_empty() => this.load_playlists(),
                        View::Favorites => this.load_favorites(),
                        _ => {}
                    }
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
                .child(Icon::new(icon).with_size(px(16.0)).text_color(if selected {
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
                    this.state.settings_open = false;
                    this.state.view = view;
                    match view {
                        View::Artists if this.state.artists.is_empty() => this.load_artists(),
                        View::Albums if this.state.albums.is_empty() => this.load_albums(),
                        View::Genres if this.state.genres.is_empty() => this.load_genres(),
                        View::Playlists if this.state.playlists.is_empty() => this.load_playlists(),
                        View::Favorites => this.load_favorites(),
                        _ => {}
                    }
                    cx.notify();
                }))
                .into_any_element()
        }
    }

    /// 侧边栏顶部的服务器胶囊：单行全圆角胶囊（状态圆点 + 当前
    /// 服务器名 + 下拉箭头），点击打开设置（切换/新增/删除都在
    /// 那里）。折叠态只显示图标按钮。
    pub(super) fn render_sidebar_server(
        &self,
        collapsed: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let connected = self.state.server.is_some();
        let label = self.active_server_label();
        let status = if self.state.loading {
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
                        .selected(self.state.settings_open)
                        .w_full()
                        .h(px(32.0))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.state.settings_open = true;
                            this.state.settings_section = SettingsSection::Servers;
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
        } else if self.state.loading {
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
                        this.state.settings_open = true;
                        this.state.settings_section = SettingsSection::Servers;
                        this.refresh_output_devices(cx);
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    pub(super) fn favorite_button(
        &self,
        kind: FavoriteKind,
        id: &str,
        cx: &Context<Self>,
    ) -> Button {
        let key = FavoriteKey::new(kind, id);
        let starred = self.state.favorite_ids.contains(&key);
        let pending = self.state.pending_favorites.contains(&key);
        let kind_name = match kind {
            FavoriteKind::Artist => "artist",
            FavoriteKind::Album => "album",
            FavoriteKind::Song => "song",
        };

        Button::new(SharedString::from(format!("favorite-{kind_name}-{id}")))
            .icon(if starred {
                AppIcon::HeartFilled
            } else {
                AppIcon::Heart
            })
            .tooltip(if starred {
                t!("Remove from favorites")
            } else {
                t!("Add to favorites")
            })
            .ghost()
            .small()
            .opacity(if pending { 0.6 } else { 1.0 })
            .rounded_full()
            .text_color(if starred {
                cx.theme().red
            } else {
                cx.theme().muted_foreground
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_favorite(key.clone());
                cx.notify();
            }))
    }

    /// 播放条模式开关的统一视觉语言：关闭时保持安静，开启时使用当前
    /// 封面强调色。与播放按钮、进度条共用色源，避免默认灰色选中态
    /// 让随机、循环、EQ 和队列看起来像普通悬停。
    pub(super) fn player_toggle_style(
        &self,
        active: bool,
        cx: &Context<Self>,
    ) -> ButtonCustomVariant {
        let accent = self.now_playing_accent(cx);
        if active {
            ButtonCustomVariant::new(cx)
                .color(accent.opacity(0.12))
                .foreground(accent)
                .hover(accent.opacity(0.18))
                .active(accent.opacity(0.24))
        } else {
            ButtonCustomVariant::new(cx)
                .color(cx.theme().transparent)
                .foreground(cx.theme().muted_foreground)
                .hover(accent.opacity(0.08))
                .active(accent.opacity(0.14))
        }
    }

    /// 随机开关按钮：开启时高亮；与循环模式相互独立。
    pub(super) fn shuffle_button(&self, cx: &Context<Self>) -> Button {
        let active = self.state.playback_modes.shuffle;
        Button::new("player-shuffle")
            .icon(AppIcon::Shuffle)
            .tooltip(if active {
                t!("Shuffle on")
            } else {
                t!("Shuffle off")
            })
            .custom(self.player_toggle_style(active, cx))
            .with_size(px(30.0))
            .rounded_full()
            .toggled(active)
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_shuffle(cx);
            }))
    }

    /// 循环模式按钮：Off → All → One 循环切换，开启时高亮，
    /// 单曲循环显示专属图标。
    pub(super) fn repeat_button(&self, cx: &Context<Self>) -> Button {
        let repeat = self.state.playback_modes.repeat;
        Button::new("player-repeat")
            .icon(if repeat == RepeatMode::One {
                AppIcon::RepeatOne
            } else {
                AppIcon::Repeat
            })
            .tooltip(repeat.tooltip())
            .custom(self.player_toggle_style(repeat != RepeatMode::Off, cx))
            .with_size(px(30.0))
            .rounded_full()
            .toggled(repeat != RepeatMode::Off)
            .on_click(cx.listener(|this, _, _, cx| {
                this.cycle_repeat(cx);
            }))
    }

    pub(super) fn now_playing_palette(&self, cx: &Context<Self>) -> (Hsla, Hsla) {
        self.state
            .now_playing
            .as_ref()
            .and_then(|song| song.cover_art.as_deref())
            .and_then(|cover_id| self.state.cover_palettes.get(cover_id).copied())
            .unwrap_or((cx.theme().info, cx.theme().chart_2))
    }

    pub(super) fn now_playing_accent(&self, cx: &Context<Self>) -> Hsla {
        let (_, accent) = self.now_playing_palette(cx);
        let accent = if accent.s < 0.16 {
            cx.theme().info
        } else {
            accent
        };
        readable_accent(accent, cx.theme().background)
    }
}
