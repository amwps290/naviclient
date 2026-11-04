//! 通用媒体库组件以及专辑、艺术家和流派网格渲染。
//! 渲染方法之间共享父模块的私有字段与业务逻辑。

use super::*;
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::popover::Popover;

impl NavidromeApp {
    pub(super) fn render_cover(
        &self,
        cover_id: Option<&str>,
        size: f32,
        _cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let cover = cover_id
            .and_then(|id| self.state.covers.get(id))
            .cloned()
            .unwrap_or_else(|| self.default_cover.clone());
        self.mark_cover_image_used(&cover);
        img(cover)
            .size(px(size))
            // Override GPUI's intrinsic image ratio so portrait artwork cannot grow the slot.
            .aspect_square()
            .object_fit(ObjectFit::Cover)
            .flex_none()
            .rounded(px(12.0))
            .into_any_element()
    }

    pub(super) fn render_vinyl_record(
        &self,
        cover_id: Option<&str>,
        size: f32,
        rotation_phase: f32,
        tonearm_engaged: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let cover = cover_id
            .and_then(|id| self.state.covers.get(id))
            .cloned()
            .unwrap_or_else(|| self.default_cover.clone());
        self.mark_cover_image_used(&cover);
        let label_size = size * 0.5;
        let label_offset = (size - label_size) * 0.5;
        let inner_cover_size = label_size - size * 0.018;
        let hole_size = size * 0.032;
        // Each tonearm layer uses (110, 110) in a 220px viewBox as its pivot.
        let tonearm_size = size * 0.94;
        let tonearm_pivot_x = size * 0.91;
        let tonearm_pivot_y = size * 0.015;
        let tonearm_left = tonearm_pivot_x - tonearm_size * 0.5;
        let tonearm_top = tonearm_pivot_y - tonearm_size * 0.5;
        let tonearm_base_size = size * 0.105;
        let tonearm_engaged_turns = 28.0 / 360.0;
        let metal_tint = self.now_playing_accent(cx);
        let metal_body = hsla(0.1, 0.025, 0.78, 1.0).blend(metal_tint.opacity(0.06));
        let cover_key = cover_id.unwrap_or("default");
        let tonearm_layer =
            |icon: AppIcon, layer: &'static str, offset_x: f32, offset_y: f32, color: Hsla| {
                Icon::new(icon)
                    .absolute()
                    .left(px(tonearm_left + offset_x))
                    .top(px(tonearm_top + offset_y))
                    .with_size(px(tonearm_size))
                    .text_color(color)
                    .with_animation(
                        SharedString::from(format!(
                            "tonearm-{layer}-{cover_key}-{tonearm_engaged}"
                        )),
                        Animation::new(Duration::from_millis(650)).with_easing(ease_out_quint()),
                        move |icon, delta| {
                            let rotation = if tonearm_engaged {
                                delta * tonearm_engaged_turns
                            } else {
                                (1.0 - delta) * tonearm_engaged_turns
                            };
                            icon.transform(Transformation::rotate(percentage(rotation)))
                        },
                    )
            };
        let tonearm_base = div()
            .absolute()
            .left(px(tonearm_pivot_x - tonearm_base_size * 0.5))
            .top(px(tonearm_pivot_y - tonearm_base_size * 0.5))
            .size(px(tonearm_base_size))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(hsla(0.1, 0.025, 0.42, 0.5))
            .bg(linear_gradient(
                132.0,
                linear_color_stop(hsla(0.1, 0.025, 0.86, 1.0), 0.0),
                linear_color_stop(
                    hsla(0.1, 0.025, 0.48, 1.0).blend(metal_tint.opacity(0.06)),
                    1.0,
                ),
            ))
            .shadow(vec![box_shadow(
                px(0.0),
                px(size * 0.006),
                px(size * 0.018),
                px(-1.0),
                hsla(0.0, 0.0, 0.0, 0.24),
            )])
            .child(
                div()
                    .size(px(tonearm_base_size * 0.72))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_1()
                    .border_color(hsla(0.0, 0.0, 1.0, 0.24))
                    .bg(linear_gradient(
                        145.0,
                        linear_color_stop(hsla(0.1, 0.025, 0.42, 1.0), 0.0),
                        linear_color_stop(hsla(0.1, 0.025, 0.22, 1.0), 1.0),
                    ))
                    .child(
                        div()
                            .size(px(tonearm_base_size * 0.28))
                            .rounded_full()
                            .border_1()
                            .border_color(hsla(0.0, 0.0, 1.0, 0.2))
                            .bg(linear_gradient(
                                135.0,
                                linear_color_stop(hsla(0.1, 0.025, 0.82, 1.0), 0.0),
                                linear_color_stop(hsla(0.1, 0.025, 0.56, 1.0), 1.0),
                            )),
                    ),
            );
        let highlight = Icon::new(AppIcon::VinylHighlight)
            .absolute()
            .top_0()
            .left_0()
            .with_size(px(size))
            .text_color(hsla(0.0, 0.0, 1.0, 0.2))
            .transform(Transformation::rotate(percentage(rotation_phase)));

        // GPUI caches the SVG mask at this size; denser grooves still use one sprite.
        let grooves = Icon::new(AppIcon::VinylGrooves)
            .absolute()
            .top_0()
            .left_0()
            .with_size(px(size))
            .text_color(hsla(0.0, 0.0, 1.0, 0.12));

        div()
            .relative()
            .w(px(size))
            .h(px(size))
            .flex_none()
            .rounded_full()
            .bg(linear_gradient(
                132.0,
                linear_color_stop(hsla(0.0, 0.0, 0.115, 1.0), 0.0),
                linear_color_stop(hsla(0.0, 0.0, 0.025, 1.0), 1.0),
            ))
            .shadow(vec![
                box_shadow(
                    px(0.0),
                    px(size * 0.008),
                    px(size * 0.015),
                    px(0.0),
                    hsla(0.0, 0.0, 0.0, 0.32),
                ),
                box_shadow(
                    px(0.0),
                    px(size * 0.035),
                    px(size * 0.08),
                    px(-size * 0.015),
                    hsla(0.0, 0.0, 0.0, 0.2),
                ),
            ])
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_full()
                    .border_1()
                    .border_color(hsla(0.0, 0.0, 0.0, 0.5)),
            )
            .child(
                div()
                    .absolute()
                    .inset(px(size * 0.006))
                    .rounded_full()
                    .border_1()
                    .border_color(hsla(0.0, 0.0, 1.0, 0.09)),
            )
            .child(highlight)
            .child(grooves)
            .child(
                div()
                    .absolute()
                    .top(px(label_offset))
                    .left(px(label_offset))
                    .size(px(label_size))
                    .rounded_full()
                    .border_1()
                    .border_color(hsla(0.0, 0.0, 1.0, 0.14))
                    .bg(linear_gradient(
                        132.0,
                        linear_color_stop(
                            hsla(0.0, 0.0, 0.12, 1.0).blend(metal_tint.opacity(0.12)),
                            0.0,
                        ),
                        linear_color_stop(hsla(0.0, 0.0, 0.035, 1.0), 1.0),
                    ))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        self.vinyl_cover
                            .render(inner_cover_size, rotation_phase)
                            .unwrap_or_else(|| {
                                img(cover)
                                    .size(px(inner_cover_size))
                                    .aspect_square()
                                    .object_fit(ObjectFit::Cover)
                                    .flex_none()
                                    .rounded_full()
                                    .into_any_element()
                            }),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px((label_size - hole_size) * 0.5))
                            .left(px((label_size - hole_size) * 0.5))
                            .size(px(hole_size))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(linear_gradient(
                                132.0,
                                linear_color_stop(hsla(0.0, 0.0, 0.62, 1.0), 0.0),
                                linear_color_stop(hsla(0.0, 0.0, 0.14, 1.0), 1.0),
                            ))
                            .border_1()
                            .border_color(hsla(0.0, 0.0, 0.0, 0.65))
                            .child(
                                div()
                                    .size(px(size * 0.012))
                                    .rounded_full()
                                    .bg(hsla(0.0, 0.0, 0.025, 1.0)),
                            ),
                    ),
            )
            .child(tonearm_layer(
                AppIcon::Tonearm,
                "shadow-close",
                size * 0.005,
                size * 0.008,
                hsla(0.0, 0.0, 0.0, 0.24),
            ))
            .child(tonearm_layer(
                AppIcon::Tonearm,
                "body",
                0.0,
                0.0,
                metal_body,
            ))
            .child(tonearm_layer(
                AppIcon::TonearmShade,
                "shade",
                0.0,
                0.0,
                hsla(0.1, 0.025, 0.12, 0.94),
            ))
            .child(tonearm_layer(
                AppIcon::TonearmHighlight,
                "highlight",
                0.0,
                0.0,
                hsla(0.0, 0.0, 1.0, 0.5),
            ))
            .child(tonearm_layer(
                AppIcon::TonearmStylus,
                "stylus-shadow",
                size * 0.005,
                size * 0.008,
                hsla(0.0, 0.0, 0.0, 0.24),
            ))
            .child(tonearm_layer(
                AppIcon::TonearmStylus,
                "stylus",
                0.0,
                0.0,
                hsla(0.0, 0.0, 1.0, 0.65),
            ))
            .child(tonearm_base)
            .into_any_element()
    }

    pub(super) fn page_header(
        &self,
        title: impl Into<SharedString>,
        subtitle: impl Into<SharedString>,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        v_flex()
            .gap_1()
            .child(
                div()
                    .text_2xl()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title.into()),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(subtitle.into()),
            )
            .into_any_element()
    }

    /// 专辑页排序菜单：当前方式打勾；切换后持久化并从第一页重新加载。
    pub(super) fn render_album_sort_button(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let app = cx.entity().downgrade();
        Button::new("albums-sort")
            .label(t!("Sort: %{mode}", mode = self.state.albums_sort.label()))
            .icon(AppIcon::ChevronDown)
            .tooltip(t!("Album sort order"))
            .ghost()
            .small()
            .dropdown_menu(move |menu, _, cx| {
                app.update(cx, |this, cx| {
                    let mut menu = menu;
                    for sort in AlbumSort::ALL {
                        let checked = this.state.albums_sort == sort;
                        menu =
                            menu.item(PopupMenuItem::new(sort.label()).checked(checked).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.set_album_sort(sort, cx);
                                }),
                            ));
                    }
                    menu
                })
                .expect("app owns album sort menu")
            })
            .into_any_element()
    }

    /// 艺术家页排序菜单：全量列表在客户端重排，切换立即生效。
    pub(super) fn render_artist_sort_button(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let app = cx.entity().downgrade();
        Button::new("artists-sort")
            .label(t!("Sort: %{mode}", mode = self.state.artists_sort.label()))
            .icon(AppIcon::ChevronDown)
            .tooltip(t!("Artist sort order"))
            .ghost()
            .small()
            .dropdown_menu(move |menu, _, cx| {
                app.update(cx, |this, cx| {
                    let mut menu = menu;
                    for sort in ArtistSort::ALL {
                        let checked = this.state.artists_sort == sort;
                        menu =
                            menu.item(PopupMenuItem::new(sort.label()).checked(checked).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.set_artist_sort(sort, cx);
                                }),
                            ));
                    }
                    menu
                })
                .expect("app owns artist sort menu")
            })
            .into_any_element()
    }

    /// 可点击的元数据文本（艺术家/专辑名）：hover 变强调色，点击执行
    /// 导航并阻止冒泡（行/卡片的整体点击是播放或打开详情）。
    /// 注意这里只阻断鼠标冒泡，与键盘输入路径无关。
    pub(super) fn clickable_meta(
        &self,
        id: impl Into<SharedString>,
        text: String,
        cx: &Context<Self>,
        open: impl Fn(&mut Self) + 'static,
    ) -> gpui_kit::AnyElement {
        div()
            .id(id.into())
            .min_w_0()
            .truncate()
            .cursor_pointer()
            .hover(|style| style.text_color(cx.theme().primary))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| {
                open(this);
                cx.notify();
            }))
            .child(text)
            .into_any_element()
    }

    /// 艺术家名文本：有 artist_id 时可点导航到艺术家页，否则静态显示。
    pub(super) fn artist_name_link(
        &self,
        id: impl Into<SharedString>,
        name: String,
        artist_id: Option<String>,
        cover_art: Option<String>,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        match artist_id.filter(|id| !id.is_empty()) {
            Some(artist_id) => self.clickable_meta(id, name.clone(), cx, move |this| {
                this.open_artist(Artist {
                    id: artist_id.clone(),
                    name: name.clone(),
                    album_count: None,
                    cover_art: cover_art.clone(),
                });
            }),
            None => div().min_w_0().truncate().child(name).into_any_element(),
        }
    }

    /// 歌曲行的专辑名文本：可点导航到专辑页。
    pub(super) fn song_album_link(
        &self,
        id: impl Into<SharedString>,
        display_name: String,
        song: &Song,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let song = song.clone();
        self.clickable_meta(id, display_name, cx, move |this| {
            this.open_song_album(&song);
        })
    }

    /// 歌曲行的艺术家名文本：可点导航到艺术家页。
    pub(super) fn song_artist_link(
        &self,
        id: impl Into<SharedString>,
        song: &Song,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let song = song.clone();
        self.clickable_meta(id, song.artist.clone(), cx, move |this| {
            this.open_song_artist(&song);
        })
    }

    /// 流派卡片网格：卡片显示流派名与专辑/歌曲数，点击进入流派详情。
    pub(super) fn render_genre_grid(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        h_flex()
            .items_start()
            .flex_wrap()
            .gap_4()
            .children(self.state.genres.iter().enumerate().map(|(index, genre)| {
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
                div()
                    .id(SharedString::from(format!("genre-card-{index}")))
                    .w(px(232.0))
                    .p_4()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .cursor_pointer()
                    .hover(|style| style.bg(cx.theme().accent.opacity(0.1)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_genre(genre_for_click.clone());
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(genre.value.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(counts),
                    )
            }))
            .into_any_element()
    }

    /// 底部播放条的均衡器弹出面板：锚定 EQ 按钮的 Popover，
    /// 点击外部自动关闭；面板内开关/预设/10 段滑杆与配置实时同步。
    pub(super) fn render_eq_popover(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let app = cx.entity().downgrade();
        let sliders = self.eq_sliders.clone();
        let enabled = self.config.equalizer.enabled;
        let gains = self.config.equalizer.gains_db;

        let presets: Vec<(&'static str, [f32; 10])> = EqSettings::PRESETS.to_vec();

        Popover::new("eq-popover")
            .trigger(
                Button::new("player-eq")
                    .icon(AppIcon::Eq)
                    .tooltip(t!("Equalizer"))
                    .custom(self.player_toggle_style(enabled, cx))
                    .with_size(px(30.0))
                    .rounded_full()
                    .toggled(enabled),
            )
            .content(move |_, _, popover_cx| {
                // accent 是背景 token，做文字/进度色在亮色主题里几乎不可见。
                let emphasis = popover_cx.theme().primary;
                let muted = popover_cx.theme().muted_foreground;
                v_flex()
                    .w(px(400.0))
                    .gap_3()
                    .p_3()
                    // 开关
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("Equalizer")),
                            )
                            .child(h_flex().gap_1().children(
                                [(true, t!("On")), (false, t!("Off"))].map(|(enable, label)| {
                                    let app = app.clone();
                                    Button::new(SharedString::from(format!("eq-popover-{enable}")))
                                        .label(label)
                                        .small()
                                        .ghost()
                                        .selected(enabled == enable)
                                        .on_click(move |_, _, cx| {
                                            let _ = app.update(cx, |this, cx| {
                                                this.set_eq_enabled(enable, cx);
                                            });
                                        })
                                }),
                            )),
                    )
                    // 预设
                    .child(h_flex().gap_1().flex_wrap().children(presets.iter().map(
                        |(name, preset_gains)| {
                            let preset_gains = *preset_gains;
                            let app = app.clone();
                            let sliders = sliders.clone();
                            Button::new(SharedString::from(format!("eq-popover-preset-{name}")))
                                .label(EqSettings::preset_label(name))
                                .small()
                                .ghost()
                                .selected(enabled && gains == preset_gains)
                                .on_click(move |_, window, cx| {
                                    for (band, slider) in sliders.iter().enumerate() {
                                        slider.update(cx, |state, cx| {
                                            state.set_value(preset_gains[band], window, cx)
                                        });
                                    }
                                    let _ = app.update(cx, |this, cx| {
                                        this.set_eq_preset_values(preset_gains, cx);
                                    });
                                })
                        },
                    )))
                    // 10 段滑杆
                    .child(
                        h_flex()
                            .justify_between()
                            .children(sliders.iter().enumerate().map(|(band, slider)| {
                                let center_hz = EqSettings::BANDS[band];
                                let label = if center_hz >= 1000.0 {
                                    format!("{}k", (center_hz / 1000.0) as u32)
                                } else {
                                    format!("{}", center_hz as u32)
                                };
                                v_flex()
                                    .items_center()
                                    .gap_1()
                                    .w(px(34.0))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(if enabled { emphasis } else { muted })
                                            .child(format!("{:+.0}", gains[band])),
                                    )
                                    .child(Slider::new(slider).vertical().h(px(96.0)))
                                    .child(div().text_xs().text_color(muted).child(label))
                            })),
                    )
                    .into_any_element()
            })
            .into_any_element()
    }

    /// 页头右侧的单视图刷新按钮：只重载当前视图，不必全量刷新媒体库。
    pub(super) fn header_refresh_button(
        &self,
        id: &'static str,
        tooltip: impl Into<SharedString>,
        loading: bool,
        cx: &Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> gpui_kit::AnyElement {
        Button::new(id)
            .icon(AppIcon::Refresh)
            .tooltip(tooltip)
            .ghost()
            .small()
            .loading(loading)
            .on_click(cx.listener(move |this, _, _, cx| {
                action(this, cx);
                cx.notify();
            }))
            .into_any_element()
    }

    /// 列表底部的“加载失败重试”行：分页请求失败时可见，点击重新请求。
    pub(super) fn render_retry_row(
        &self,
        id: &'static str,
        cx: &Context<Self>,
        action: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> gpui_kit::AnyElement {
        div()
            .py_2()
            .flex()
            .justify_center()
            .child(
                Button::new(id)
                    .label(t!("Loading failed — try again"))
                    .outline()
                    .small()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        action(this, cx);
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    pub(super) fn render_scrolling_title(
        &self,
        text: SharedString,
        viewport_width: f32,
        id: SharedString,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        self.render_scrolling_text(
            text,
            viewport_width,
            rems(0.875).to_pixels(window.rem_size()),
            FontWeight::MEDIUM,
            id,
            window,
        )
    }

    pub(super) fn render_scrolling_text(
        &self,
        text: SharedString,
        viewport_width: f32,
        font_size: Pixels,
        weight: FontWeight,
        id: SharedString,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        let text_style = window.text_style().highlight(weight);
        let text_width = {
            let cache = self.title_width_cache.borrow();
            let key = (f32::from(font_size).to_bits(), weight, text.clone());
            if let Some(&width) = cache.get(&key) {
                width
            } else {
                drop(cache);
                let width = window
                    .text_system()
                    .shape_line(
                        text.clone(),
                        font_size,
                        &[text_style.to_run(text.len())],
                        None,
                    )
                    .width;
                let mut cache = self.title_width_cache.borrow_mut();
                if cache.len() > 20_000 {
                    cache.clear();
                }
                cache.insert(key, width);
                width
            }
        };
        let viewport_width = px(viewport_width);

        if text_width <= viewport_width {
            return div()
                .w(viewport_width)
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(font_size)
                .font_weight(weight)
                .child(text)
                .into_any_element();
        }

        let gap = px(32.0);
        let travel = text_width + gap;
        let travel_px = f32::from(travel);
        let duration_seconds = (travel_px / 28.0_f32 + 1.5_f32).max(4.0_f32);
        let hold_fraction = (1.5_f32 / duration_seconds).clamp(0.0_f32, 0.8_f32);
        let animation = Animation::new(Duration::from_secs_f32(duration_seconds))
            .repeat()
            .with_easing(move |delta| {
                if delta <= hold_fraction {
                    0.0
                } else {
                    (delta - hold_fraction) / (1.0 - hold_fraction)
                }
            });
        let repeated_text = text.clone();

        div()
            .w(viewport_width)
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .child(
                h_flex()
                    .flex_none()
                    .gap(gap)
                    .text_size(font_size)
                    .font_weight(weight)
                    .child(text)
                    .child(repeated_text)
                    .with_animation(id, animation, move |this, delta| {
                        this.ml(px(-travel_px * delta))
                    }),
            )
            .into_any_element()
    }

    pub(super) fn render_album_grid(
        &self,
        scope: &str,
        albums: &[Album],
        cx: &Context<Self>,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        let grid_start = std::time::Instant::now();
        let grid =
            h_flex()
                .items_start()
                .flex_wrap()
                .gap_5()
                .children(albums.iter().enumerate().map(|(index, album)| {
                    let key = item_instance_key(scope, "album", index, &album.id);
                    let album_for_click = album.clone();
                    let album_for_play = album.clone();
                    let album_for_cover = album.clone();
                    let album_id_for_hover = key.clone();
                    v_flex()
                        .id(SharedString::from(key.clone()))
                        .w(px(176.0))
                        .gap_2()
                        .child(
                            div()
                                .id(SharedString::from(format!("album-cover-{}", album.id)))
                                .relative()
                                .w(px(176.0))
                                .h(px(176.0))
                                .cursor_pointer()
                                .on_hover(cx.listener(move |this, hovering, _, cx| {
                                    this.set_hovered(&album_id_for_hover, *hovering, cx);
                                }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_album(album_for_cover.clone());
                                    cx.notify();
                                }))
                                .child(self.render_cover(album.cover_art.as_deref(), 176.0, cx))
                                .child(div().absolute().top_2().right_2().child(
                                    self.favorite_button(FavoriteKind::Album, &album.id, cx),
                                ))
                                .when(
                                    self.state.hovered_item.as_deref() == Some(key.as_str()),
                                    |this| {
                                        this.child(
                                            div()
                                                .absolute()
                                                .inset_0()
                                                .rounded(px(12.0))
                                                .bg(hsla(0.0, 0.0, 0.0, 0.32))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                    cx.stop_propagation()
                                                })
                                                .child(
                                                    div()
                                                        .id(SharedString::from(format!(
                                                            "album-play-{}",
                                                            album.id
                                                        )))
                                                        .size(px(48.0))
                                                        .rounded_full()
                                                        .bg(hsla(0.0, 0.0, 1.0, 0.92))
                                                        .flex()
                                                        .items_center()
                                                        .justify_center()
                                                        .shadow_md()
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.play_album(
                                                                    album_for_play.clone(),
                                                                );
                                                                cx.notify();
                                                            },
                                                        ))
                                                        .child(
                                                            Icon::new(PlayerIcon::Play)
                                                                .size(px(22.0))
                                                                .text_color(hsla(
                                                                    0.0, 0.0, 0.12, 1.0,
                                                                )),
                                                        ),
                                                ),
                                        )
                                    },
                                ),
                        )
                        .child(
                            Button::new(SharedString::from(format!("album-{}", album.id)))
                                .ghost()
                                .w_full()
                                .justify_start()
                                .child(self.render_scrolling_title(
                                    album.name.clone().into(),
                                    144.0,
                                    SharedString::from(format!("album-title-{}", album.id)),
                                    window,
                                ))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_album(album_for_click.clone());
                                    cx.notify();
                                })),
                        )
                        .child(
                            h_flex()
                                .px_2()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
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
                }));
        let grid_elapsed = grid_start.elapsed();
        if grid_elapsed > Duration::from_millis(8) {
            log::warn!(
                "render_album_grid built {} albums in {grid_elapsed:?}",
                albums.len()
            );
        }
        grid.into_any_element()
    }

    /// 专辑网格骨架屏：数据加载期间占位，避免空页面跳动。
    pub(super) fn render_album_skeleton_grid(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let muted = cx.theme().muted_foreground;
        h_flex()
            .items_start()
            .flex_wrap()
            .gap_5()
            .children((0..12).map(|_| {
                v_flex()
                    .w(px(176.0))
                    .gap_2()
                    .child(
                        div()
                            .w(px(176.0))
                            .h(px(176.0))
                            .rounded_md()
                            .bg(muted.opacity(0.08)),
                    )
                    .child(
                        div()
                            .w(px(128.0))
                            .h(px(12.0))
                            .rounded_full()
                            .bg(muted.opacity(0.1)),
                    )
                    .child(
                        div()
                            .w(px(88.0))
                            .h(px(10.0))
                            .rounded_full()
                            .bg(muted.opacity(0.07)),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }

    /// 列表行骨架屏：播放列表 / 歌曲列表加载期间占位。
    pub(super) fn render_list_skeleton(
        &self,
        rows: usize,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let muted = cx.theme().muted_foreground;
        v_flex()
            .gap_2()
            .children((0..rows).map(|_| {
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .w(px(40.0))
                            .h(px(40.0))
                            .rounded_md()
                            .bg(muted.opacity(0.08)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_1()
                            .child(
                                div()
                                    .h(px(12.0))
                                    .w_full()
                                    .max_w(px(240.0))
                                    .rounded_full()
                                    .bg(muted.opacity(0.1)),
                            )
                            .child(
                                div()
                                    .h(px(10.0))
                                    .w(px(120.0))
                                    .rounded_full()
                                    .bg(muted.opacity(0.07)),
                            ),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }

    /// 艺术家封面网格骨架屏：与 artist 页的圆角方形封面一致。
    pub(super) fn render_artist_skeleton_grid(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let muted = cx.theme().muted_foreground;
        h_flex()
            .items_start()
            .flex_wrap()
            .gap_5()
            .children((0..15).map(|_| {
                v_flex()
                    .w(px(152.0))
                    .gap_2()
                    .child(
                        div()
                            .w(px(152.0))
                            .h(px(152.0))
                            .rounded_lg()
                            .bg(muted.opacity(0.08)),
                    )
                    .child(
                        div()
                            .w(px(104.0))
                            .h(px(12.0))
                            .rounded_full()
                            .bg(muted.opacity(0.1)),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }

    pub(super) fn render_artist_grid(
        &self,
        artists: &[Artist],
        cx: &Context<Self>,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        h_flex()
            .items_start()
            .flex_wrap()
            .gap_5()
            .children(artists.iter().enumerate().map(|(index, artist)| {
                let key = item_instance_key("artists", "artist", index, &artist.id);
                let artist_for_click = artist.clone();
                let artist_for_play = artist.clone();
                let artist_for_cover = artist.clone();
                let artist_id_for_hover = key.clone();
                v_flex()
                    .id(SharedString::from(key.clone()))
                    .w(px(152.0))
                    .gap_2()
                    .child(
                        div()
                            .id(SharedString::from(format!("artist-cover-{}", artist.id)))
                            .relative()
                            .w(px(152.0))
                            .h(px(152.0))
                            .cursor_pointer()
                            .on_hover(cx.listener(move |this, hovering, _, cx| {
                                this.set_hovered(&artist_id_for_hover, *hovering, cx);
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_artist(artist_for_cover.clone());
                                cx.notify();
                            }))
                            .child(self.render_cover(artist.cover_art.as_deref(), 152.0, cx))
                            .child(
                                div()
                                    .absolute()
                                    .top_2()
                                    .right_2()
                                    .child(self.favorite_button(
                                        FavoriteKind::Artist,
                                        &artist.id,
                                        cx,
                                    )),
                            )
                            .when(
                                self.state.hovered_item.as_deref() == Some(key.as_str()),
                                |this| {
                                    this.child(
                                        div()
                                            .absolute()
                                            .inset_0()
                                            .rounded(px(12.0))
                                            .bg(hsla(0.0, 0.0, 0.0, 0.32))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(
                                                div()
                                                    .id(SharedString::from(format!(
                                                        "artist-play-{}",
                                                        artist.id
                                                    )))
                                                    .size(px(48.0))
                                                    .rounded_full()
                                                    .bg(hsla(0.0, 0.0, 1.0, 0.92))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .shadow_md()
                                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                        cx.stop_propagation()
                                                    })
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        cx.stop_propagation();
                                                        this.play_artist(artist_for_play.clone());
                                                        cx.notify();
                                                    }))
                                                    .child(
                                                        Icon::new(PlayerIcon::Play)
                                                            .size(px(22.0))
                                                            .text_color(hsla(0.0, 0.0, 0.12, 1.0)),
                                                    ),
                                            ),
                                    )
                                },
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("artist-{}", artist.id)))
                            .ghost()
                            .w_full()
                            .justify_start()
                            .child(self.render_scrolling_title(
                                artist.name.clone().into(),
                                120.0,
                                SharedString::from(format!("artist-title-{}", artist.id)),
                                window,
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.open_artist(artist_for_click.clone());
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .px_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                artist
                                    .album_count
                                    .map(|count| t!("%{count} albums", count = count))
                                    .unwrap_or_else(|| t!("Artist")),
                            ),
                    )
            }))
            .into_any_element()
    }
}
