//! 正在播放页、首页以及主内容区路由的渲染。

use super::*;

/// Six 176px album cards with five 20px gaps: Home previews intentionally
/// stop at six items, so the listening grid should share the same visual edge
/// instead of stretching indefinitely on ultrawide/fullscreen windows.
const HOME_PREVIEW_MAX_WIDTH: f32 = 6.0 * 176.0 + 5.0 * 20.0;

impl NavidromeApp {
    /// 渲染当前播放页，包括黑胶封面、歌词和歌曲操作。
    pub(super) fn render_now_playing(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(song) = &self.state.now_playing else {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    div()
                        .text_xl()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(t!("No track playing")),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("Choose a song from your library.")),
                )
                .into_any_element();
        };

        let viewport_width = f32::from(window.viewport_size().width);
        let viewport_height = f32::from(window.viewport_size().height);
        let max_cover_size = if viewport_width < 1_100.0 {
            300.0
        } else {
            380.0
        };
        let cover_size = (viewport_height - 300.0).clamp(220.0, max_cover_size);
        let playback = self.audio.state();
        let accent = self.now_playing_accent(cx);
        let radio_seed = song.clone();
        let radio_loading = self.song_radio_loading(&song.id);
        let active_line = self
            .state
            .lyrics
            .as_ref()
            .and_then(|lyrics| lyrics.active_line_index(playback.position));
        let lyrics = self.state.lyrics.as_ref();
        let lyrics_body = if self.state.lyrics_loading {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child(t!("Loading lyrics..."))
                .into_any_element()
        } else if let Some(error) = &self.state.lyrics_error {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(cx.theme().muted_foreground)
                .child(t!("Lyrics are unavailable for this track."))
                .child(div().text_xs().child(error.clone()))
                .into_any_element()
        } else if let Some(lyrics) = lyrics.filter(|lyrics| !lyrics.lines.is_empty()) {
            let synced = lyrics.is_synced();
            let show_translation = self.config.show_lyrics_translation;
            v_flex()
                .id("lyrics-scroll")
                .flex_1()
                .min_h_0()
                .px_6()
                .overflow_y_scroll()
                .track_scroll(&self.lyrics_scroll_handle)
                .child(div().h(px(240.0)).flex_none())
                .children(lyrics.lines.iter().enumerate().map(|(index, lyric_line)| {
                    let current = synced && active_line == Some(index);
                    let distance = active_line
                        .map(|active| active.abs_diff(index))
                        .unwrap_or(usize::MAX);
                    let text = lyric_line.text.clone();
                    let start_ms = lyric_line.start_ms;
                    let translation = lyric_line
                        .translation
                        .clone()
                        .filter(|text| show_translation && !text.is_empty());
                    let line = div()
                        .id(SharedString::from(format!(
                            "lyric-line-{}-{index}",
                            song.id
                        )))
                        .w_full()
                        .max_w(px(760.0))
                        .min_h(px(64.0))
                        .mx_auto()
                        .px_4()
                        .py_2()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_center()
                        .line_height(rems(1.45))
                        .when_some(start_ms, |this, start_ms| {
                            this.cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.seek_playback(Duration::from_millis(start_ms));
                                    cx.notify();
                                }))
                        })
                        // 原文与翻译作为一个整体参与高亮/点击；翻译行小号弱化色。
                        .child(v_flex().gap_1().child(div().child(text)).when_some(
                            translation,
                            |this, translation| {
                                this.child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(translation),
                                )
                            },
                        ));

                    if current {
                        line.text_2xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(accent)
                            .with_animation(
                                SharedString::from(format!("active-lyric-{}-{index}", song.id)),
                                Animation::new(Duration::from_millis(220))
                                    .with_easing(ease_out_quint()),
                                |this, delta| this.opacity(0.64 + delta * 0.36),
                            )
                            .into_any_element()
                    } else if synced {
                        let opacity = match distance {
                            1 => 0.68,
                            2 => 0.52,
                            3 => 0.4,
                            _ => 0.28,
                        };
                        line.text_lg()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().foreground.opacity(opacity))
                            .into_any_element()
                    } else {
                        line.text_lg()
                            .text_color(cx.theme().foreground.opacity(0.72))
                            .into_any_element()
                    }
                }))
                .child(div().h(px(280.0)).flex_none())
                .into_any_element()
        } else {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child(t!("No lyrics are available for this track."))
                .into_any_element()
        };

        let rotation_phase = self.vinyl_cover.phase();
        h_flex()
            .size_full()
            .min_h_0()
            .px_8()
            .py_5()
            .gap_8()
            .items_center()
            .child(
                v_flex()
                    .w(px((cover_size + 40.0).max(320.0)))
                    .flex_none()
                    .items_center()
                    .gap_4()
                    .child(self.render_vinyl_record(
                        song.cover_art.as_deref(),
                        cover_size,
                        rotation_phase,
                        playback.active && !playback.paused,
                        cx,
                    ))
                    .child(
                        v_flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(song.title.clone()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(self.song_artist_link(
                                        SharedString::from(format!(
                                            "now-playing-artist-{}",
                                            song.id
                                        )),
                                        song,
                                        cx,
                                    )),
                            )
                            .when(!song.album.is_empty(), |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(self.song_album_link(
                                            SharedString::from(format!(
                                                "now-playing-album-{}",
                                                song.id
                                            )),
                                            song.album.clone(),
                                            song,
                                            cx,
                                        )),
                                )
                            })
                            .child(
                                Button::new("now-playing-song-radio")
                                    .icon(PlayerIcon::Play)
                                    .label(if radio_loading {
                                        t!("Building song radio…")
                                    } else {
                                        t!("Start song radio")
                                    })
                                    .outline()
                                    .disabled(self.api.is_none() || radio_loading)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.start_song_radio(radio_seed.clone());
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .pl_6()
                    .child(
                        v_flex()
                            .items_center()
                            .gap_1()
                            .pb_1()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("Lyrics")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground.opacity(0.82))
                                    .child(t!(
                                        "%{title} - %{artist}",
                                        title = song.title,
                                        artist = song.artist
                                    )),
                            ),
                    )
                    .child(lyrics_body),
            )
            .into_any_element()
    }

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

    fn render_home(&self, window: &mut Window, cx: &Context<Self>) -> gpui_kit::AnyElement {
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

    pub(super) fn render_content(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        match self.state.view {
            View::Home => self.render_home(window, cx),
            View::Favorites => {
                let favorites = &self.state.favorites;
                let total =
                    favorites.artists.len() + favorites.albums.len() + favorites.songs.len();
                let mut content = v_flex().gap_5().child(
                    h_flex()
                        .w_full()
                        .items_end()
                        .gap_3()
                        .child(div().flex_1().child(self.page_header(
                            t!("Favorites"),
                            t!(
                                "%{total} starred items from your Navidrome library",
                                total = total
                            ),
                            cx,
                        )))
                        .child(self.header_refresh_button(
                            "refresh-favorites",
                            t!("Refresh favorites"),
                            false,
                            cx,
                            |this, _cx| this.load_favorites(),
                        )),
                );

                if total == 0 {
                    content =
                        content.child(div().py_8().text_color(cx.theme().muted_foreground).child(
                            t!("Click a heart on a song, album, or artist to add it here."),
                        ));
                }
                if !favorites.artists.is_empty() {
                    content = content
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(t!("Artists")),
                        )
                        .child(self.render_artist_grid(&favorites.artists, cx, window));
                }
                if !favorites.albums.is_empty() {
                    content = content
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(t!("Albums")),
                        )
                        .child(self.render_album_grid("favorites", &favorites.albums, cx, window));
                }
                if !favorites.songs.is_empty() {
                    content = content
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(t!("Songs")),
                        )
                        .child(self.render_song_list("favorites", false, cx));
                }

                content.into_any_element()
            }
            View::Artists => v_flex()
                .gap_5()
                .child(
                    h_flex()
                        .w_full()
                        .items_end()
                        .gap_3()
                        .child(div().flex_1().child(self.page_header(
                            t!("Artists"),
                            t!(
                                "%{count} artists in your library",
                                count = self.state.artists.len()
                            ),
                            cx,
                        )))
                        .child(self.render_artist_sort_button(cx))
                        .child(self.header_refresh_button(
                            "refresh-artists",
                            t!("Refresh artists"),
                            self.state.loading && self.state.artists.is_empty(),
                            cx,
                            |this, _cx| this.load_artists(),
                        )),
                )
                .child(if self.state.artists.is_empty() && self.state.loading {
                    self.render_artist_skeleton_grid(cx)
                } else {
                    let visible = self.state.artists_visible.min(self.state.artists.len());
                    self.render_artist_grid(&self.state.artists[..visible], cx, window)
                })
                .into_any_element(),
            View::Albums => v_flex()
                .gap_5()
                .child(
                    h_flex()
                        .w_full()
                        .items_end()
                        .gap_3()
                        .child(div().flex_1().child(self.page_header(
                            t!("Albums"),
                            t!(
                                "%{count} albums in your library",
                                count = self.state.albums.len()
                            ),
                            cx,
                        )))
                        .child(self.render_album_sort_button(cx))
                        .child(self.header_refresh_button(
                            "refresh-albums",
                            t!("Refresh albums"),
                            self.state.albums_loading,
                            cx,
                            |this, _cx| this.load_albums(),
                        )),
                )
                .child(
                    if self.state.albums.is_empty() && self.state.albums_loading {
                        self.render_album_skeleton_grid(cx)
                    } else {
                        self.render_album_grid("albums", &self.state.albums, cx, window)
                    },
                )
                .when(
                    self.state.albums_page_failed && !self.state.albums_loading,
                    |this| {
                        this.child(self.render_retry_row("retry-albums", cx, |this, _cx| {
                            this.load_more_albums()
                        }))
                    },
                )
                .into_any_element(),
            View::Genres => {
                // 有旧数据（磁盘缓存）时直接渲染，仅在后台重验证中提示；
                // 首次无缓存才显示骨架屏。
                let subtitle = if self.state.genres_loading && !self.state.genres.is_empty() {
                    t!(
                        "%{count} genres — refreshing…",
                        count = self.state.genres.len()
                    )
                } else {
                    t!(
                        "%{count} genres in your library",
                        count = self.state.genres.len()
                    )
                };
                v_flex()
                    .gap_5()
                    .child(
                        h_flex()
                            .w_full()
                            .items_end()
                            .gap_3()
                            .child(div().flex_1().child(self.page_header(
                                t!("Genres"),
                                subtitle,
                                cx,
                            )))
                            .child(self.header_refresh_button(
                                "refresh-genres",
                                t!("Refresh genres"),
                                self.state.genres_loading,
                                cx,
                                |this, _cx| this.load_genres(),
                            )),
                    )
                    .child(
                        if self.state.genres.is_empty() && self.state.genres_loading {
                            self.render_album_skeleton_grid(cx)
                        } else if self.state.genres.is_empty() {
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("No genres in your library yet."))
                                .into_any_element()
                        } else {
                            self.render_genre_grid(cx)
                        },
                    )
                    .into_any_element()
            }
            View::GenreDetail => {
                let Some(genre) = self.state.current_genre.clone() else {
                    return div().child(t!("Genre not found")).into_any_element();
                };
                v_flex()
                    .gap_5()
                    .child(self.page_header(
                        genre,
                        t!("%{count} albums", count = self.state.genre_albums.len()),
                        cx,
                    ))
                    .child(
                        if self.state.genre_albums.is_empty() && self.state.genre_albums_loading {
                            self.render_album_skeleton_grid(cx)
                        } else if self.state.genre_albums.is_empty() {
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("No albums in this genre yet."))
                                .into_any_element()
                        } else {
                            self.render_album_grid(
                                "genre-detail",
                                &self.state.genre_albums,
                                cx,
                                window,
                            )
                        },
                    )
                    .when(
                        self.state.genre_albums_page_failed && !self.state.genre_albums_loading,
                        |this| {
                            this.child(self.render_retry_row(
                                "retry-genre-albums",
                                cx,
                                |this, _cx| this.load_more_genre_albums(),
                            ))
                        },
                    )
                    .into_any_element()
            }
            View::Playlists => {
                let visible_playlists =
                    self.state.playlists_visible.min(self.state.playlists.len());
                let playlist_list = v_flex()
                    .w_full()
                    .border_t_1()
                    .border_b_1()
                    .border_color(cx.theme().border.opacity(0.72))
                    .children(
                        self.state.playlists[..visible_playlists]
                            .iter()
                            .enumerate()
                            .map(|(index, playlist)| {
                                let playlist_for_click = playlist.clone();
                                let track_count = playlist.song_count.unwrap_or_default();
                                let details = playlist
                                    .owner
                                    .as_deref()
                                    .filter(|owner| !owner.trim().is_empty())
                                    .map(|owner| {
                                        t!(
                                            "%{track_count} tracks | %{owner}",
                                            track_count = track_count,
                                            owner = owner
                                        )
                                    })
                                    .unwrap_or_else(|| {
                                        t!("%{track_count} tracks", track_count = track_count)
                                    });

                                h_flex()
                                    .id(SharedString::from(format!("playlist-row-{}", playlist.id)))
                                    .h(px(68.0))
                                    .px_3()
                                    .gap_3()
                                    .items_center()
                                    .cursor_pointer()
                                    .when(index > 0, |this| {
                                        this.border_t_1()
                                            .border_color(cx.theme().border.opacity(0.52))
                                    })
                                    .hover(|style| style.bg(cx.theme().list_hover))
                                    .child(self.render_cover(
                                        playlist.cover_art.as_deref(),
                                        48.0,
                                        cx,
                                    ))
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .truncate()
                                                    .child(playlist.name.clone()),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .truncate()
                                                    .child(details),
                                            ),
                                    )
                                    .child(
                                        Icon::new(AppIcon::ChevronRight)
                                            .small()
                                            .text_color(cx.theme().muted_foreground),
                                    )
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.open_playlist(playlist_for_click.clone());
                                        cx.notify();
                                    }))
                            }),
                    );

                v_flex()
                    .gap_5()
                    .child(
                        h_flex()
                            .w_full()
                            .items_end()
                            .gap_3()
                            .flex_wrap()
                            .child(
                                div().flex_1().min_w(px(240.0)).child(self.page_header(
                                    t!("Playlists"),
                                    t!(
                                        "%{count} playlists",
                                        count = self.state.playlists.len()
                                    ),
                                    cx,
                                )),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        Button::new("new-playlist")
                                            .label(t!("New playlist"))
                                            .primary()
                                            .small()
                                            .disabled(
                                                self.playlist_ui.busy || self.api.is_none(),
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.name_playlist(None, vec![], window, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new("import-playlist")
                                            .label(t!("Import playlist"))
                                            .outline()
                                            .small()
                                            .disabled(self.api.is_none() || self.playlist_ui.busy)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.open_import_dialog(window, cx);
                                            })),
                                    )
                                    .child(self.header_refresh_button(
                                        "refresh-playlists",
                                        t!("Refresh playlists"),
                                        self.playlist_ui.list_loading,
                                        cx,
                                        |this, _cx| this.load_playlists(),
                                    )),
                            ),
                    )
                    .when(
                        self.state.playlists.is_empty() && self.playlist_ui.list_loading,
                        |this| this.child(self.render_list_skeleton(8, cx)),
                    )
                    .when(
                        self.state.playlists.is_empty() && !self.playlist_ui.list_loading,
                        |this| {
                            this.child(
                                v_flex()
                                    .w_full()
                                    .min_h(px(180.0))
                                    .items_center()
                                    .justify_center()
                                    .gap_2()
                                    .px_6()
                                    .text_center()
                                    .child(
                                        Icon::new(AppIcon::Playlists)
                                            .with_size(px(28.0))
                                            .text_color(
                                                cx.theme().muted_foreground.opacity(0.72),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(t!("No playlists are available.")),
                                    )
                                    .child(
                                        div()
                                            .max_w(px(420.0))
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(t!(
                                                "Create a playlist or import one to organize your music."
                                            )),
                                    ),
                            )
                        },
                    )
                    .when(!self.state.playlists.is_empty(), |this| {
                        this.child(playlist_list)
                    })
                    .into_any_element()
            }
            View::Search => {
                let mut content = v_flex()
                    .gap_5()
                    .child(self.page_header(
                        t!("Search"),
                        t!("Find music across the entire server library."),
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .child(Input::new(&self.search_input).w_full()),
                            )
                            .child(
                                Button::new("run-search")
                                    .label(t!("Search"))
                                    .primary()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.submit_search(cx);
                                        cx.notify();
                                    })),
                            )
                            .child(self.header_refresh_button(
                                "refresh-search",
                                t!("Run search again"),
                                false,
                                cx,
                                |this, cx| this.submit_search(cx),
                            )),
                    );
                if self.state.search_results.is_some() {
                    // 过滤只在有结果后出现；会话内状态，不持久化。
                    content =
                        content.child(h_flex().gap_2().children(SearchFilter::ALL.map(|filter| {
                            Button::new(SharedString::from(format!(
                                "search-filter-{}",
                                filter.label()
                            )))
                            .label(filter.label())
                            .ghost()
                            .small()
                            .selected(self.state.search_filter == filter)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.set_search_filter(filter, cx);
                                },
                            ))
                        })));
                }
                if let Some(results) = &self.state.search_results {
                    let filter = self.state.search_filter;
                    let show_artists =
                        filter == SearchFilter::All || filter == SearchFilter::Artists;
                    let show_albums = filter == SearchFilter::All || filter == SearchFilter::Albums;
                    let show_songs = filter == SearchFilter::All || filter == SearchFilter::Songs;
                    if show_artists {
                        if !results.artists.is_empty() {
                            content = content
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(t!("Artists")),
                                )
                                .child(self.render_artist_grid(&results.artists, cx, window));
                        } else if filter == SearchFilter::Artists {
                            content = content.child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("No artists matched your search.")),
                            );
                        }
                    }
                    if show_albums {
                        if !results.albums.is_empty() {
                            content = content
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(t!("Albums")),
                                )
                                .child(self.render_album_grid(
                                    "search",
                                    &results.albums,
                                    cx,
                                    window,
                                ));
                        } else if filter == SearchFilter::Albums {
                            content = content.child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("No albums matched your search.")),
                            );
                        }
                    }
                    if show_songs {
                        if !results.songs.is_empty() {
                            content = content
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(t!("Songs")),
                                )
                                .child(self.render_song_list("search", false, cx));
                        } else if filter == SearchFilter::Songs {
                            content = content.child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("No songs matched your search.")),
                            );
                        }
                    }
                    if filter == SearchFilter::All
                        && results.artists.is_empty()
                        && results.albums.is_empty()
                        && results.songs.is_empty()
                    {
                        content = content.child(
                            div()
                                .py_8()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("No results matched your search.")),
                        );
                    }
                }
                content.into_any_element()
            }
            View::ArtistDetail => {
                let Some(artist) = &self.state.current_artist else {
                    return div().child(t!("Artist not found")).into_any_element();
                };
                let popular_count = self.state.artist_top_songs.len();
                let artist_for_play_all = artist.clone();
                let artist_radio_loading = self.artist_radio_loading(&artist.id);
                v_flex()
                    .gap_5()
                    .child(
                        h_flex()
                            .w_full()
                            .gap_4()
                            .items_end()
                            .flex_wrap()
                            .child(self.render_cover(artist.cover_art.as_deref(), 112.0, cx))
                            .child(div().flex_1().min_w(px(220.0)).child(self.page_header(
                                artist.name.clone(),
                                t!(
                                    "%{albums} albums · %{songs} songs",
                                    albums = self.state.artist_albums.len(),
                                    songs = popular_count
                                ),
                                cx,
                            )))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .flex_wrap()
                                    .child(
                                        Button::new("play-artist-all")
                                            .label(t!("Play all"))
                                            .primary()
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.play_artist(artist_for_play_all.clone());
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new("play-artist-popular")
                                            .label(t!("Play popular"))
                                            .outline()
                                            .disabled(popular_count == 0)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.play_artist_top_songs();
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new("start-artist-radio")
                                            .label(if artist_radio_loading {
                                                t!("Building artist radio…")
                                            } else {
                                                t!("Start artist radio")
                                            })
                                            .outline()
                                            .disabled(popular_count == 0 || artist_radio_loading)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.start_artist_radio();
                                                cx.notify();
                                            })),
                                    )
                                    .child(self.favorite_button(
                                        FavoriteKind::Artist,
                                        &artist.id,
                                        cx,
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t!("Popular songs")),
                    )
                    .child(if self.state.artist_top_songs_loading {
                        h_flex()
                            .h(px(96.0))
                            .w_full()
                            .items_center()
                            .justify_center()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("Loading popular songs…"))
                            .into_any_element()
                    } else if let Some(error) = &self.state.artist_top_songs_error {
                        h_flex()
                            .w_full()
                            .gap_3()
                            .items_center()
                            .rounded_md()
                            .border_1()
                            .border_color(cx.theme().border)
                            .p_3()
                            .child(
                                div()
                                    .flex_1()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!(
                                        "Popular songs are unavailable: %{error}",
                                        error = error
                                    )),
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
                    } else if self.state.artist_top_songs.is_empty() {
                        div()
                            .py_4()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("No popular songs were returned for this artist."))
                            .into_any_element()
                    } else {
                        self.render_song_list("artist-top", false, cx)
                    })
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t!("Albums")),
                    )
                    .child(self.render_album_grid(
                        "artist-detail",
                        &self.state.artist_albums,
                        cx,
                        window,
                    ))
                    .into_any_element()
            }
            View::AlbumDetail => {
                let Some(album) = &self.state.current_album else {
                    return div().child(t!("Album not found")).into_any_element();
                };
                v_flex()
                    .h_full()
                    .min_h_0()
                    .gap_5()
                    .child(
                        h_flex()
                            .gap_5()
                            .items_end()
                            .flex_none()
                            .child(self.render_cover(album.cover_art.as_deref(), 180.0, cx))
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_2xl()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(album.name.clone()),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_1()
                                            .text_color(cx.theme().muted_foreground)
                                            .min_w_0()
                                            .child(self.artist_name_link(
                                                "album-detail-artist",
                                                album.artist.clone(),
                                                album.artist_id.clone(),
                                                album.cover_art.clone(),
                                                cx,
                                            ))
                                            .when_some(album.year, |this, year| {
                                                this.child(t!("- %{year}", year = year))
                                            }),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .flex_wrap()
                                            .child(
                                                Button::new("play-album")
                                                    .label(t!("Play album"))
                                                    .primary()
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        let songs =
                                                            this.state.current_songs.clone();
                                                        if !songs.is_empty() {
                                                            this.play_song_list(&songs, 0);
                                                        }
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                Button::new("append-album")
                                                    .icon(AppIcon::Queue)
                                                    .label(t!("Append all"))
                                                    .outline()
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        let songs =
                                                            this.state.current_songs.clone();
                                                        this.append_all(&songs);
                                                        cx.notify();
                                                    })),
                                            )
                                            .child(
                                                Button::new("album-add-to-playlist")
                                                    .label(t!("Add to playlist…"))
                                                    .outline()
                                                    .disabled(
                                                        self.playlist_ui.busy
                                                            || self.state.current_songs.is_empty(),
                                                    )
                                                    .on_click(cx.listener(
                                                        |this, _, window, cx| {
                                                            this.choose_playlist(
                                                                crate::playlist::song_ids(
                                                                    &this.state.current_songs,
                                                                ),
                                                                window,
                                                                cx,
                                                            );
                                                        },
                                                    )),
                                            ),
                                    ),
                            ),
                    )
                    .child(self.render_song_list("album-detail", false, cx))
                    .into_any_element()
            }
            View::PlaylistDetail => {
                let Some(playlist) = &self.state.current_playlist else {
                    return div().child(t!("Playlist not found")).into_any_element();
                };
                let rename = playlist.clone();
                let delete = playlist.clone();
                let read_only = !crate::playlist::can_edit(playlist, &self.config.username);
                let edit_disabled =
                    read_only || self.playlist_ui.busy || self.playlist_ui.detail_loading;
                v_flex()
                    .h_full()
                    .min_h_0()
                    .gap_5()
                    .child(self.page_header(
                        playlist.name.clone(),
                        t!("%{count} tracks", count = self.state.playlist_songs.len()),
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                Button::new("play-playlist")
                                    .icon(PlayerIcon::Play)
                                    .label(t!("Play all"))
                                    .small()
                                    .info()
                                    .outline()
                                    .disabled(
                                        self.state.playlist_songs.is_empty()
                                            || self.playlist_ui.detail_loading,
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let songs = this.state.playlist_songs.clone();
                                        if !songs.is_empty() {
                                            this.play_song_list_with_origin(
                                                &songs,
                                                0,
                                                QueueOrigin::Playlist,
                                            );
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("append-playlist")
                                    .icon(AppIcon::Queue)
                                    .label(t!("Append all"))
                                    .small()
                                    .outline()
                                    .disabled(
                                        self.state.playlist_songs.is_empty()
                                            || self.playlist_ui.detail_loading,
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let songs = this.state.playlist_songs.clone();
                                        this.append_all_with_origin(&songs, QueueOrigin::Playlist);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("rename-playlist")
                                    .label(t!("Rename"))
                                    .small()
                                    .outline()
                                    .disabled(edit_disabled)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.name_playlist(Some(rename.clone()), vec![], window, cx)
                                    })),
                            )
                            .child(
                                Button::new("delete-playlist")
                                    .label(t!("Delete"))
                                    .small()
                                    .ghost()
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
                                        self.playlist_ui.busy || self.playlist_ui.detail_loading,
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if let Some(p) = &this.state.current_playlist {
                                            this.load_playlist_detail(p.id.clone());
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(if self.playlist_ui.busy {
                                t!("Saving playlist…")
                            } else if self.playlist_ui.detail_loading {
                                t!("Loading playlist…")
                            } else if read_only {
                                t!("Shared playlist · Only the owner can edit it.")
                            } else if self.state.playlist_songs.is_empty() {
                                t!("This playlist is empty. Use a song’s actions menu to add tracks.")
                            } else {
                                t!("Use a song’s actions menu to move or remove it.")
                            }),
                    )
                    .child(self.render_song_list("playlist-detail", false, cx))
                    .into_any_element()
            }
            View::NowPlaying => self.render_now_playing(window, cx),
        }
    }
}
