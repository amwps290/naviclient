//! 首页焦点内容、每日推荐及专辑预览区块渲染。

use super::*;

/// Six 176px album cards with five 20px gaps: Home previews intentionally
/// stop at six items, so the listening grid should share the same visual edge
/// instead of stretching indefinitely on ultrawide/fullscreen windows.
const HOME_PREVIEW_MAX_WIDTH: f32 = 6.0 * 176.0 + 5.0 * 20.0;

impl NavidromeApp {
    /// Home 页可折叠区块：标题行（点击折叠/展开）+ 内容。后续新增区块只需传入唯一 id 与内容。
    pub(super) fn render_home_section(
        &self,
        id: &'static str,
        title: impl Into<SharedString>,
        content: gpui_kit::AnyElement,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let title: SharedString = title.into();
        let collapsed = self.state.collapsed_sections.contains(id);
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .w_full()
                    .h(px(38.0))
                    .items_center()
                    .justify_between()
                    .cursor_pointer()
                    .id(SharedString::from(format!("home-section-{id}")))
                    .border_b_1()
                    .border_color(cx.theme().border.opacity(0.68))
                    .hover(|style| style.text_color(cx.theme().primary))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_home_section(id, cx);
                    }))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title.clone()),
                    )
                    .child(
                        Icon::new(if collapsed {
                            AppIcon::ChevronRight
                        } else {
                            AppIcon::ChevronDown
                        })
                        .size(px(15.0))
                        .text_color(cx.theme().muted_foreground),
                    ),
            )
            .when(!collapsed, |this| this.child(content))
            .into_any_element()
    }

    fn home_feature_colors(
        &self,
        cover_id: Option<&str>,
        cx: &Context<Self>,
    ) -> (Hsla, Hsla, Hsla) {
        let (base, raw_accent) = cover_id
            .and_then(|id| self.state.cover_palettes.get(id).copied())
            .unwrap_or((cx.theme().info, cx.theme().chart_2));
        // Grayscale artwork otherwise produces another gray action button.
        // Blend in the theme's semantic info color while retaining cover identity.
        let raw_accent = if raw_accent.s < 0.14 {
            cx.theme().info.blend(raw_accent.opacity(0.35))
        } else {
            raw_accent
        };
        let light = cx.theme().background.l >= 0.5;
        let start = cx
            .theme()
            .secondary
            .blend(base.opacity(if light { 0.24 } else { 0.18 }));
        let end = cx
            .theme()
            .background
            .blend(raw_accent.opacity(if light { 0.18 } else { 0.14 }));
        (
            start,
            end,
            readable_accent(raw_accent, cx.theme().background),
        )
    }

    /// Home 的唯一主焦点：优先继续当前播放；无播放会话时展示最近专辑。
    /// 使用既有封面调色板与播放器按钮语言，不引入新的品牌色或装饰体系。
    fn render_home_feature(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        let compact = compact_window_layout(f32::from(window.viewport_size().width));
        let art_size = if compact { 96.0 } else { 136.0 };
        let feature_height = if compact { 144.0 } else { 176.0 };

        if let Some(song) = &self.state.now_playing {
            let playback = self.audio.state();
            let is_playing = playback.active && !playback.paused;
            let playback_label = if is_playing { t!("Pause") } else { t!("Play") };
            let (surface_start, surface_end, accent) =
                self.home_feature_colors(song.cover_art.as_deref(), cx);
            let play_style = ButtonCustomVariant::new(cx)
                .color(accent)
                .foreground(accent_foreground(accent))
                .hover(adjust_lightness(accent, 0.05))
                .active(adjust_lightness(accent, -0.05));
            return Some(
                h_flex()
                    .id("home-continue-listening")
                    .w_full()
                    .h(px(feature_height))
                    .when(compact, |this| this.p_4().gap_4())
                    .when(!compact, |this| this.p_5().gap_5())
                    .items_center()
                    .rounded(px(12.0))
                    .border_1()
                    .border_color(cx.theme().border.opacity(0.58))
                    .shadow_sm()
                    .bg(linear_gradient(
                        118.0,
                        linear_color_stop(surface_start, 0.0),
                        linear_color_stop(surface_end, 1.0),
                    ))
                    .child(
                        div()
                            .id("home-feature-cover")
                            .flex_none()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.open_now_playing();
                                cx.notify();
                            }))
                            .child(self.render_cover(song.cover_art.as_deref(), art_size, cx)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .when(compact, |this| this.gap_1())
                            .when(!compact, |this| this.gap_2())
                            .child(
                                div()
                                    .when(compact, |this| this.text_sm())
                                    .when(!compact, |this| this.text_base())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(accent)
                                    .child(t!("Continue listening")),
                            )
                            .child(
                                div()
                                    .when(compact, |this| this.text_xl())
                                    .when(!compact, |this| this.text_2xl())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(song.title.clone()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(cx.theme().foreground.opacity(0.72))
                                    .truncate()
                                    .child(if song.album.trim().is_empty() {
                                        song.artist.clone()
                                    } else {
                                        format!("{} · {}", song.artist, song.album)
                                    }),
                            )
                            .child(
                                h_flex()
                                    .when(compact, |this| this.pt_1().gap_1())
                                    .when(!compact, |this| this.pt_2().gap_2())
                                    .items_center()
                                    .child(
                                        Button::new("home-feature-playback")
                                            .icon(if is_playing {
                                                PlayerIcon::Pause
                                            } else {
                                                PlayerIcon::Play
                                            })
                                            .accessibility_label(playback_label.clone())
                                            .tooltip(playback_label)
                                            .custom(play_style)
                                            .rounded(px(999.0))
                                            .w(px(if compact { 40.0 } else { 44.0 }))
                                            .h(px(if compact { 40.0 } else { 44.0 }))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.toggle_playback();
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new("home-feature-now-playing")
                                            .label(t!("Open player"))
                                            .icon(AppIcon::ChevronRight)
                                            .ghost()
                                            .small()
                                            .rounded(px(8.0))
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.open_now_playing();
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    )
                    .into_any_element(),
            );
        }

        let album = self.state.recent_albums.first()?.clone();
        let (surface_start, surface_end, accent) =
            self.home_feature_colors(album.cover_art.as_deref(), cx);
        let play_style = ButtonCustomVariant::new(cx)
            .color(accent)
            .foreground(accent_foreground(accent))
            .hover(adjust_lightness(accent, 0.05))
            .active(adjust_lightness(accent, -0.05));
        let album_for_cover = album.clone();
        let album_for_play = album.clone();
        let album_for_open = album.clone();
        Some(
            h_flex()
                .id("home-recent-feature")
                .w_full()
                .h(px(feature_height))
                .when(compact, |this| this.p_4().gap_4())
                .when(!compact, |this| this.p_5().gap_5())
                .items_center()
                .rounded(px(12.0))
                .border_1()
                .border_color(cx.theme().border.opacity(0.58))
                .shadow_sm()
                .bg(linear_gradient(
                    118.0,
                    linear_color_stop(surface_start, 0.0),
                    linear_color_stop(surface_end, 1.0),
                ))
                .child(
                    div()
                        .id("home-feature-album-cover")
                        .flex_none()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_album(album_for_cover.clone());
                            cx.notify();
                        }))
                        .child(self.render_cover(album.cover_art.as_deref(), art_size, cx)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .when(compact, |this| this.gap_1())
                        .when(!compact, |this| this.gap_2())
                        .child(
                            div()
                                .when(compact, |this| this.text_sm())
                                .when(!compact, |this| this.text_base())
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(accent)
                                .child(t!("Recently played")),
                        )
                        .child(
                            div()
                                .when(compact, |this| this.text_xl())
                                .when(!compact, |this| this.text_2xl())
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(album.name.clone()),
                        )
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(cx.theme().foreground.opacity(0.72))
                                .truncate()
                                .child(album.artist.clone()),
                        )
                        .child(
                            h_flex()
                                .when(compact, |this| this.pt_1().gap_1())
                                .when(!compact, |this| this.pt_2().gap_2())
                                .items_center()
                                .child(
                                    Button::new("home-feature-play-album")
                                        .icon(PlayerIcon::Play)
                                        .accessibility_label(t!("Play album"))
                                        .tooltip(t!("Play album"))
                                        .custom(play_style)
                                        .rounded(px(999.0))
                                        .w(px(if compact { 40.0 } else { 44.0 }))
                                        .h(px(if compact { 40.0 } else { 44.0 }))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.play_album(album_for_play.clone());
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new("home-feature-open-album")
                                        .label(t!("Go to album"))
                                        .icon(AppIcon::ChevronRight)
                                        .ghost()
                                        .small()
                                        .rounded(px(8.0))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.open_album(album_for_open.clone());
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }

    fn render_home_primary_section(
        &self,
        title: impl Into<SharedString>,
        content: gpui_kit::AnyElement,
    ) -> gpui_kit::AnyElement {
        v_flex()
            .gap_3()
            .child(
                div()
                    .text_xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title.into()),
            )
            .child(content)
            .into_any_element()
    }

    /// Home 页专辑区块的通用内容：加载骨架 / 空状态提示 / 专辑网格。
    pub(super) fn render_home_album_content(
        &self,
        scope: &str,
        albums: &[Album],
        loading: bool,
        empty_text: impl Into<SharedString>,
        cx: &Context<Self>,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        let empty_text: SharedString = empty_text.into();
        if albums.is_empty() && loading {
            self.render_album_skeleton_grid(cx)
        } else if albums.is_empty() {
            v_flex()
                .w_full()
                .min_h(px(132.0))
                .items_center()
                .justify_center()
                .gap_2()
                .text_center()
                .child(
                    Icon::new(AppIcon::Albums)
                        .with_size(px(28.0))
                        .text_color(cx.theme().muted_foreground.opacity(0.72)),
                )
                .child(
                    div()
                        .max_w(px(420.0))
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(empty_text),
                )
                .into_any_element()
        } else {
            self.render_album_grid(scope, albums, cx, window)
        }
    }

    pub(super) fn toggle_home_section(&mut self, id: &'static str, cx: &mut Context<Self>) {
        if self.state.collapsed_sections.contains(id) {
            self.state.collapsed_sections.remove(id);
        } else {
            self.state.collapsed_sections.insert(id.to_string());
        }
        cx.notify();
    }

    fn render_daily_mix_card(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let compact = compact_window_layout(f32::from(window.viewport_size().width));
        let feature_height = if compact { 144.0 } else { 176.0 };
        let count = self
            .state
            .daily_mix
            .as_ref()
            .map_or(0, |snapshot| snapshot.tracks.len());
        let subtitle = if self.state.daily_mix_loading {
            t!("Creating your daily mix…")
        } else if let Some(error) = &self.state.daily_mix_error {
            t!("Daily mix unavailable: %{error}", error = error)
        } else if count > 0 {
            t!("%{count} tracks selected for today", count = count)
        } else {
            t!("A balanced mix of favorites, familiar artists, and new discoveries.")
        };
        let mix_cover = self.state.daily_mix.as_ref().and_then(|snapshot| {
            snapshot
                .tracks
                .iter()
                .find_map(|track| track.song.cover_art.as_deref())
        });
        let (surface_start, surface_end, accent) = self.home_feature_colors(mix_cover, cx);
        let play_style = ButtonCustomVariant::new(cx)
            .color(accent)
            .foreground(accent_foreground(accent))
            .hover(adjust_lightness(accent, 0.05))
            .active(adjust_lightness(accent, -0.05));
        v_flex()
            .w_full()
            .h(px(feature_height))
            .justify_between()
            .gap_3()
            .rounded(px(12.0))
            .border_1()
            .border_color(cx.theme().border.opacity(0.58))
            .shadow_sm()
            .bg(linear_gradient(
                118.0,
                linear_color_stop(surface_start, 0.0),
                linear_color_stop(surface_end, 1.0),
            ))
            .when(compact, |this| this.p_4())
            .when(!compact, |this| this.p_5())
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .size(px(if compact { 40.0 } else { 48.0 }))
                            .flex_none()
                            .rounded(px(8.0))
                            .bg(accent.opacity(0.12))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                Icon::new(AppIcon::Shuffle)
                                    .with_size(px(if compact { 18.0 } else { 22.0 }))
                                    .text_color(accent),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .when(compact, |this| this.text_base())
                                    .when(!compact, |this| this.text_lg())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("Daily Mix")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .truncate()
                                    .text_color(if self.state.daily_mix_error.is_some() {
                                        cx.theme().danger
                                    } else {
                                        cx.theme().foreground.opacity(0.72)
                                    })
                                    .child(subtitle),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_1()
                    .when(count == 0, |this| {
                        this.child(
                            Button::new("daily-mix-create")
                                .label(if self.state.daily_mix_loading {
                                    t!("Creating…")
                                } else if self.state.daily_mix_error.is_some() {
                                    t!("Retry")
                                } else {
                                    t!("Create and play")
                                })
                                .custom(play_style)
                                .rounded(px(8.0))
                                .small()
                                .disabled(self.state.daily_mix_loading)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if this.state.daily_mix_error.is_some() {
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
                            Button::new("daily-mix-play")
                                .icon(PlayerIcon::Play)
                                .accessibility_label(t!("Play"))
                                .tooltip(t!("Play"))
                                .custom(play_style)
                                .rounded(px(999.0))
                                .w(px(if compact { 40.0 } else { 44.0 }))
                                .h(px(if compact { 40.0 } else { 44.0 }))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.play_daily_mix();
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("daily-mix-append")
                                .icon(AppIcon::Queue)
                                .tooltip(t!("Add to queue"))
                                .ghost()
                                .small()
                                .rounded(px(8.0))
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
                                .rounded(px(8.0))
                                .disabled(self.state.daily_mix_loading)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.refresh_daily_mix();
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .into_any_element()
    }

    pub(super) fn render_home(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let feature_uses_recent =
            self.state.now_playing.is_none() && !self.state.recent_albums.is_empty();
        let recent_start = usize::from(feature_uses_recent);
        let recent_end = (recent_start + 6).min(self.state.recent_albums.len());
        let recent_preview = &self.state.recent_albums[recent_start..recent_end];
        let frequent_preview =
            &self.state.frequent_albums[..self.state.frequent_albums.len().min(6)];
        let newest_preview = &self.state.newest_albums[..self.state.newest_albums.len().min(6)];
        let random_preview = &self.state.random_albums[..self.state.random_albums.len().min(6)];
        let status = if self.state.server.is_some() {
            format!("{} · {}", self.active_server_label(), t!("Connected"))
        } else {
            self.state.status.clone()
        };
        let listening_feature = self.render_home_feature(window, cx);
        let daily_mix_card = self.render_daily_mix_card(window, cx);

        v_flex()
            .gap_6()
            .child(self.page_header(t!("Home"), status, cx))
            .child(
                h_flex()
                    .w_full()
                    .max_w(px(HOME_PREVIEW_MAX_WIDTH))
                    .gap_4()
                    .items_stretch()
                    .when_some(listening_feature, |this, feature| {
                        this.child(div().flex_1().min_w_0().child(feature))
                    })
                    .child(div().flex_1().min_w_0().child(daily_mix_card)),
            )
            .when(
                !recent_preview.is_empty() || self.state.recent_albums_loading,
                |this| {
                    this.child(self.render_home_primary_section(
                        t!("Recently played"),
                        self.render_home_album_content(
                            "recent",
                            recent_preview,
                            self.state.recent_albums_loading,
                            t!("Nothing played yet — recently played albums will appear here."),
                            cx,
                            window,
                        ),
                    ))
                },
            )
            .child(self.render_home_section(
                "frequent",
                t!("Frequently played"),
                self.render_home_album_content(
                    "frequent",
                    frequent_preview,
                    self.state.frequent_albums_loading,
                    t!("Nothing frequently played yet — your most-played albums will appear here."),
                    cx,
                    window,
                ),
                cx,
            ))
            .child(self.render_home_section(
                "newest",
                t!("Newest albums"),
                self.render_home_album_content(
                    "newest",
                    newest_preview,
                    self.state.newest_albums_loading,
                    t!("No albums in your library yet."),
                    cx,
                    window,
                ),
                cx,
            ))
            .child(self.render_home_section(
                "random",
                t!("Random albums"),
                self.render_home_album_content(
                    "random",
                    random_preview,
                    self.state.random_albums_loading,
                    t!("No random albums to show — refresh the library for a fresh pick."),
                    cx,
                    window,
                ),
                cx,
            ))
            .into_any_element()
    }
}
