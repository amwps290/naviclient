//! 全部视图渲染：歌曲列表、专辑/艺术家网格、播放器栏、
//! 正在播放页、设置覆盖层、迷你播放器、内容区路由。
//! 从 app.rs 拆出；渲染方法之间共享父模块的私有字段与逻辑方法。

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
            .rounded_lg()
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
                                                .rounded_lg()
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
                                            .rounded_lg()
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
                                                    .on_click(cx.listener(move |this, _, _, cx| {
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

    pub(super) fn render_playing_indicator(
        &self,
        song_id: &str,
        current: bool,
        animated: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        if !current {
            if self.state.hovered_item.as_deref() == Some(song_id) {
                return Icon::new(PlayerIcon::Play)
                    .size(px(15.0))
                    .text_color(cx.theme().muted_foreground)
                    .into_any_element();
            }
            return div().w(px(16.0)).h(px(16.0)).flex_none().into_any_element();
        }

        let bar_heights = [7.0_f32, 13.0_f32, 9.0_f32];
        h_flex()
            .w(px(16.0))
            .h(px(16.0))
            .flex_none()
            .items_end()
            .justify_center()
            .gap(px(2.0))
            .children(bar_heights.into_iter().enumerate().map(|(index, height)| {
                let bar = div()
                    .w(px(3.0))
                    .h(px(height))
                    .rounded_full()
                    .bg(cx.theme().info);

                if animated {
                    let phase = index as f32 * 0.23;
                    bar.with_animation(
                        SharedString::from(format!("playing-{song_id}-{index}")),
                        Animation::new(Duration::from_millis(720))
                            .repeat()
                            .with_easing(move |delta| {
                                let shifted = (delta + phase) % 1.0;
                                1.0 - (shifted * 2.0 - 1.0).abs()
                            }),
                        |bar, delta| bar.h(px(4.0 + delta * 10.0)),
                    )
                    .into_any_element()
                } else {
                    bar.opacity(0.7).into_any_element()
                }
            }))
            .into_any_element()
    }

    pub(super) fn render_song_list(
        &self,
        scope: &'static str,
        editable: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        // uniform_list 视口虚拟化：只构建可见行，长列表不再一次性挂载全部元素，
        // 滚动多久元素树就只有多大，与列表总长度无关。
        let Some(songs) = self.songs_for_scope(scope) else {
            return div().into_any_element();
        };
        if songs.is_empty() {
            return v_flex()
                .w_full()
                .flex_1()
                .min_h(px(180.0))
                .items_center()
                .justify_center()
                .gap_2()
                .px_6()
                .text_center()
                .child(
                    Icon::new(AppIcon::Playlists)
                        .with_size(px(28.0))
                        .text_color(cx.theme().muted_foreground.opacity(0.72)),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(t!("No tracks here yet")),
                )
                .child(
                    div()
                        .max_w(px(360.0))
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("Tracks will appear here when they are available.")),
                )
                .into_any_element();
        }
        let handle = self.song_list_scroll(scope);
        let list_width = f32::from(self.content_scroll_handle.bounds().size.width);
        let queue_drawer = scope == "queue" && self.state.queue_drawer_open;
        let compact_columns = queue_drawer || compact_song_columns(list_width);
        // 专辑 / 播放列表详情和队列页中表格占满剩余高度（表头固定，列表内部滚动）；
        // 收藏与搜索页的歌曲表格与其他分区共存，限制高度独立滚动。
        let fill = matches!(scope, "album-detail" | "playlist-detail" | "queue");
        let rows = cx.processor(move |this, range: Range<usize>, _window, cx| {
            let playback = this.audio.state();
            range
                .map(|index| {
                    this.render_song_row(scope, index, editable, compact_columns, &playback, cx)
                })
                .collect::<Vec<_>>()
        });
        // 限高模式：显式给出列表高度 min(行数×行高, 视口)，不依赖 max_h。
        // taffy 计算自动高度父容器的内容尺寸时会忽略 max 约束（ContentSize
        // 模式不套用 min/max），Infer+max_h 会让列表按全部内容高度参与布局。
        let viewport = self.content_scroll_handle.bounds().size.height;
        let cap = if viewport > px(0.0) {
            (viewport - px(36.0)).max(px(300.0))
        } else {
            px(600.0)
        };
        let capped_height = px((songs.len() as f32 * SONG_ROW_HEIGHT).min(f32::from(cap)));
        let list = uniform_list(
            SharedString::from(format!("song-list-{scope}")),
            songs.len(),
            rows,
        )
        .track_scroll(&handle)
        .on_scroll_wheel(cx.listener(|this, _: &ScrollWheelEvent, _, _| {
            this.maybe_load_visible_song_covers();
        }))
        .when(fill, |list| list.flex_1().min_h_0())
        .when(!fill, |list| list.h(capped_height));
        v_flex()
            .relative()
            .w_full()
            .overflow_hidden()
            .when(fill, |this| this.flex_1().min_h_0())
            .when(self.multi_selection_count(scope) > 0, |this| {
                this.child(self.render_multi_action_bar(scope, cx))
            })
            .when(!queue_drawer, |this| {
                this.child(
                    h_flex()
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
                        .child(div().flex_1().flex_grow(3.0).child(t!("Title")))
                        .when(!compact_columns, |this| {
                            this.child(
                                div()
                                    .flex_1()
                                    .flex_grow(2.0)
                                    .max_w(px(180.0))
                                    .child(t!("Artist")),
                            )
                        })
                        .child(
                            div()
                                .w(px(if compact_columns { 48.0 } else { 64.0 }))
                                .text_right()
                                .child(t!("Time")),
                        )
                        .child(div().w(px(match (editable, compact_columns) {
                            (true, true) | (false, true) => 60.0,
                            (true, false) => 124.0,
                            (false, false) => 100.0,
                        }))),
                )
            })
            .child(list)
            .child(Scrollbar::vertical(&handle))
            .into_any_element()
    }

    /// 打开“曲目信息”对话框。对话框挂在窗口全局层，菜单入口先让
    /// 菜单完成关闭再挂载，避免其焦点恢复吞掉 Enter/Escape。
    pub(super) fn open_song_info(
        &mut self,
        song: Song,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.song_info = Some(song);
        if !window.has_active_dialog(cx) {
            let app = cx.entity().downgrade();
            let render_app = app.clone();
            window.defer(cx, move |window, cx| {
                window.open_dialog(cx, move |dialog, _, cx| {
                    render_app
                        .update(cx, |this, cx| this.render_song_info_dialog(dialog, cx))
                        .expect("app owns song info dialog")
                });
            });
        }
        cx.notify();
    }

    /// 曲目信息对话框：服务器元数据全览（标签、音轨号、时长、格式、
    /// 码率、文件大小、路径），只读。
    fn render_song_info_dialog(
        &self,
        dialog: gpui_kit::component::dialog::Dialog,
        cx: &Context<Self>,
    ) -> gpui_kit::component::dialog::Dialog {
        let Some(song) = &self.state.song_info else {
            return dialog;
        };
        let info_row = |content: gpui_kit::Div, label: &str, value: String| {
            content.child(
                h_flex()
                    .gap_3()
                    .items_baseline()
                    .child(
                        div()
                            .w(px(96.0))
                            .flex_none()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(label.to_string()),
                    )
                    .child(div().flex_1().min_w_0().text_sm().truncate().child(value)),
            )
        };
        let mut info = v_flex().gap_2();
        info = info_row(info, &t!("Title"), song.title.clone());
        if !song.artist.trim().is_empty() {
            info = info_row(info, &t!("Artist"), song.artist.clone());
        }
        if !song.album.trim().is_empty() {
            info = info_row(info, &t!("Album"), song.album.clone());
        }
        if song.track.is_some() || song.disc.is_some() {
            let value = match (song.disc, song.track) {
                (Some(disc), Some(track)) => t!(
                    "Disc %{disc} · Track %{track}",
                    disc = disc,
                    track = format!("{track:02}")
                ),
                (Some(disc), None) => t!("Disc %{disc}", disc = disc),
                (None, Some(track)) => t!("Track %{track}", track = format!("{track:02}")),
                (None, None) => std::borrow::Cow::Borrowed(""),
            };
            info = info_row(info, &t!("Position"), value.to_string());
        }
        info = info_row(info, &t!("Duration"), format_duration(song.duration));
        if song.suffix.as_deref().is_some_and(|s| !s.is_empty()) {
            let mut value = song.suffix.as_deref().unwrap_or_default().to_uppercase();
            if let Some(bit_rate) = song.bit_rate {
                value.push_str(&format!(" · {bit_rate} kbps"));
            }
            if let Some(size) = song.size {
                value.push_str(&format!(" · {}", format_file_size(size.max(0) as u64)));
            }
            info = info_row(info, &t!("Format"), value.to_string());
        }
        if let Some(path) = &song.path {
            if !path.is_empty() {
                info = info_row(info, &t!("File"), path.clone());
            }
        }
        info = info_row(info, &t!("ID"), song.id.clone());
        dialog
            .title(t!("Track info"))
            .w(px(440.0))
            .close_button(true)
            .keyboard(true)
            .on_close(cx.listener(|this, _, _, cx| {
                this.state.song_info = None;
                cx.notify();
            }))
            .child(
                v_flex().gap_2().p_1().child(info).child(
                    div()
                        .pt_2()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("Metadata from the Navidrome server.")),
                ),
            )
    }

    /// 多选操作条：显示所选数量与批量动作。收藏按钮按“是否全部
    /// 已收藏”智能切换方向。所有动作复用单曲操作的批量版本。
    pub(super) fn render_multi_action_bar(
        &self,
        scope: &'static str,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let songs = self.selected_songs(scope);
        let all_favorited = songs.iter().all(|song| {
            self.state
                .favorite_ids
                .contains(&FavoriteKey::new(FavoriteKind::Song, &song.id))
        });
        let favorite_icon = if all_favorited {
            AppIcon::HeartFilled
        } else {
            AppIcon::Heart
        };
        let favorite_label = if all_favorited {
            t!("Unfavorite selected")
        } else {
            t!("Favorite selected")
        };
        let add_disabled = self.playlist_ui.busy || self.api.is_none();
        h_flex()
            .h(px(44.0))
            .flex_none()
            .w_full()
            .px_3()
            .gap_2()
            .items_center()
            .bg(cx.theme().list_active)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(cx.theme().primary)
                    .child(t!("%{count} selected", count = songs.len())),
            )
            .child(div().flex_1())
            .child(
                Button::new(SharedString::from(format!("multi-play-{scope}")))
                    .icon(PlayerIcon::Play)
                    .tooltip(t!("Play selected (Enter)"))
                    .ghost()
                    .small()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.play_selected_songs(scope);
                        cx.notify();
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("multi-next-{scope}")))
                    .icon(PlayerIcon::Next)
                    .tooltip(t!("Play selected next"))
                    .ghost()
                    .small()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected_play_next(scope);
                        cx.notify();
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("multi-queue-{scope}")))
                    .icon(AppIcon::Queue)
                    .tooltip(t!("Add selected to queue"))
                    .ghost()
                    .small()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected_to_queue(scope);
                        cx.notify();
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("multi-playlist-{scope}")))
                    .label(t!("Add to playlist…"))
                    .disabled(add_disabled)
                    .ghost()
                    .small()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        let ids: Vec<String> = this
                            .selected_songs(scope)
                            .iter()
                            .map(|song| song.id.clone())
                            .collect();
                        this.choose_playlist(ids, window, cx);
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("multi-favorite-{scope}")))
                    .icon(favorite_icon)
                    .tooltip(favorite_label)
                    .ghost()
                    .small()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.favorite_selected(scope);
                        cx.notify();
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("multi-clear-{scope}")))
                    .icon(AppIcon::Close)
                    .tooltip(t!("Clear selection (Esc)"))
                    .ghost()
                    .small()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.clear_song_multi();
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// 渲染歌曲列表中的一行。行内事件只捕获 (scope, index)，
    /// 触发时再从当前状态解析列表与歌曲，避免每次渲染克隆整个列表。
    pub(super) fn render_song_row(
        &self,
        scope: &'static str,
        index: usize,
        editable: bool,
        compact_columns: bool,
        playback: &PlaybackState,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let queue_drawer = scope == "queue" && self.state.queue_drawer_open;
        let Some(song) = self.song_for_scope(scope, index) else {
            return div().into_any_element();
        };
        let total = self.songs_for_scope(scope).map_or(0, <[Song]>::len);
        let key = item_instance_key(scope, "song", index, &song.id);
        let menu_app = cx.entity().downgrade();
        let button_menu_app = menu_app.clone();
        let current = self
            .state
            .now_playing
            .as_ref()
            .is_some_and(|playing| playing.id == song.id)
            && (!editable || self.state.queue_index == Some(index));
        let animated = current && playback.active && !playback.paused;
        let row_accent = self.now_playing_accent(cx);
        let row_accent_foreground = accent_foreground(row_accent);
        let album = if song.album.trim().is_empty() {
            t!("Unknown album").to_string()
        } else {
            song.album.clone()
        };
        let keyboard_selected =
            self.state
                .song_list_selected
                .is_some_and(|(selected_scope, selected_index)| {
                    selected_scope == scope && selected_index == index
                });
        let multi_selected = self
            .state
            .song_list_multi
            .as_ref()
            .is_some_and(|multi| multi.scope == scope && multi.indices.contains(&index));
        // 普通悬浮只使用主题的中性列表表面；主题色保留给多选、键盘
        // 光标和正在播放等有明确含义的状态。已带状态的行悬浮时只略微
        // 加深原有底色，避免鼠标经过反而削弱选中反馈。
        let hover_background = if multi_selected {
            row_accent.opacity(0.2)
        } else if keyboard_selected {
            row_accent.opacity(0.13)
        } else if current {
            row_accent.opacity(0.1)
        } else {
            cx.theme().list_hover
        };
        // uniform_list 把每行当作独立根元素布局（Definite 可用宽度），
        // 不会像普通容器那样横向拉伸，必须显式 w_full：否则行宽收缩为
        // 内容宽度，行尾按钮、悬停/正在播放高亮都无法纵向对齐。
        let row = h_flex()
            .id(SharedString::from(key.clone()))
            .w_full()
            .h(px(if queue_drawer { 64.0 } else { 60.0 }))
            .px_3()
            .gap(if queue_drawer { px(8.0) } else { px(12.0) })
            .relative()
            .when(index > 0, |this| {
                this.border_t_1()
                    .border_color(cx.theme().border.opacity(0.52))
            })
            .hover(move |style| style.bg(hover_background))
            .cursor_pointer();
        // 高亮优先级：多选 > 列表光标 > 正在播放。列表选择继承当前
        // 封面强调色，让媒体库与正在播放页使用同一种音乐氛围。
        let row = if multi_selected {
            row.bg(row_accent.opacity(0.16))
        } else if keyboard_selected {
            row.bg(row_accent.opacity(0.1))
        } else if current {
            row.bg(row_accent.opacity(0.07))
        } else {
            row
        };
        let position_indicator = if multi_selected {
            div()
                .size(px(22.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_md()
                .bg(row_accent)
                .child(
                    Icon::new(KitIcon::Check)
                        .with_size(px(13.0))
                        .text_color(row_accent_foreground),
                )
                .into_any_element()
        } else if queue_drawer && current {
            self.render_playing_indicator(&key, true, animated, cx)
        } else {
            div()
                .w_full()
                .text_xs()
                .text_center()
                .font_weight(if keyboard_selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if keyboard_selected || current {
                    row_accent
                } else {
                    cx.theme().muted_foreground
                })
                .child((index + 1).to_string())
                .into_any_element()
        };
        row.child(
            div()
                .w(px(if queue_drawer { 20.0 } else { 28.0 }))
                .h_full()
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(position_indicator),
        )
        .child(
            div()
                .w(px(42.0))
                .h(px(42.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(self.render_cover(song.cover_art.as_deref(), 40.0, cx)),
        )
        .child(
            h_flex()
                .flex_1()
                .flex_grow(3.0)
                .min_w_0()
                .gap_2()
                .text_color(if current {
                    row_accent
                } else {
                    cx.theme().foreground
                })
                .when(!queue_drawer, |this| {
                    this.child(self.render_playing_indicator(&key, current, animated, cx))
                })
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap(px(2.0))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .line_height(rems(1.25))
                                .truncate()
                                .child(song.title.clone()),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .line_height(rems(1.2))
                                .when(queue_drawer, |this| {
                                    this.child(self.song_artist_link(
                                        SharedString::from(format!("song-row-artist-{key}")),
                                        song,
                                        cx,
                                    ))
                                })
                                .when(!queue_drawer, |this| {
                                    this.child(self.song_album_link(
                                        SharedString::from(format!("song-row-album-{key}")),
                                        album,
                                        song,
                                        cx,
                                    ))
                                }),
                        ),
                ),
        )
        .when(!compact_columns, |row| {
            row.child(
                div()
                    .flex_1()
                    .flex_grow(2.0)
                    .max_w(px(180.0))
                    .min_w_0()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.song_artist_link(
                        SharedString::from(format!("song-row-artist-{key}")),
                        song,
                        cx,
                    )),
            )
        })
        .child(
            div()
                .w(px(if queue_drawer {
                    40.0
                } else if compact_columns {
                    48.0
                } else {
                    64.0
                }))
                .text_sm()
                .text_right()
                .text_color(cx.theme().muted_foreground)
                .child(format_duration(song.duration)),
        )
        .child(
            // 常规列表只保留收藏、播放与更多操作；“下一首/加入队列”等
            // 低频动作仍完整保留在右键和省略号菜单中。
            h_flex()
                .flex_none()
                .h_full()
                .w(px(match (editable, compact_columns) {
                    (true, true) | (false, true) => 60.0,
                    (true, false) => 124.0,
                    (false, false) => 100.0,
                }))
                .items_center()
                .justify_end()
                .gap_0p5()
                .when(editable && !compact_columns, |slot| {
                    slot.child(
                        Button::new(SharedString::from(format!("queue-up-{key}")))
                            .icon(AppIcon::ChevronUp)
                            .tooltip(t!("Move up"))
                            .ghost()
                            .small()
                            .disabled(index == 0)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.move_queue_item(index, index - 1);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("queue-down-{key}")))
                            .icon(AppIcon::ChevronDown)
                            .tooltip(t!("Move down"))
                            .ghost()
                            .small()
                            .disabled(index + 1 >= total)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.move_queue_item(index, index + 1);
                                cx.notify();
                            })),
                    )
                })
                .when(editable, |slot| {
                    slot.child(
                        Button::new(SharedString::from(format!("queue-remove-{key}")))
                            .icon(AppIcon::Close)
                            .tooltip(t!("Remove from queue"))
                            .ghost()
                            .small()
                            .text_color(cx.theme().muted_foreground)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove_from_queue(index);
                                cx.notify();
                            })),
                    )
                })
                .when(!editable && !compact_columns, |slot| {
                    slot.child(self.favorite_button(FavoriteKind::Song, &song.id, cx))
                })
                .when(!editable, |slot| {
                    slot.child(
                        Button::new(SharedString::from(format!("play-song-{key}")))
                            .icon(PlayerIcon::Play)
                            .tooltip(t!("Play"))
                            .ghost()
                            .small()
                            .text_color(row_accent)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select_song_row(scope, index);
                                this.play_song_at_scope(scope, index);
                                cx.notify();
                            })),
                    )
                })
                .child(
                    div()
                        .id("song-actions-trigger")
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(|_, _, cx| cx.stop_propagation())
                        .child(
                            Button::new(SharedString::from(format!("song-menu-{key}")))
                                .icon(IconName::Ellipsis)
                                .tooltip(t!("Song actions"))
                                .ghost()
                                .small()
                                .dropdown_menu(move |menu, _, cx| {
                                    button_menu_app
                                        .update(cx, |this, cx| {
                                            let Some(context) =
                                                this.song_menu_context(scope, index, editable)
                                            else {
                                                return menu;
                                            };
                                            this.song_menu(menu, context, cx)
                                        })
                                        .expect("app owns song menu")
                                }),
                        ),
                ),
        )
        .on_click(
            cx.listener(move |this, event: &gpui_kit::ClickEvent, _, cx| {
                match song_row_click_action(event.modifiers(), event.click_count()) {
                    SongRowClickAction::ToggleMulti => this.toggle_song_multi(scope, index),
                    SongRowClickAction::RangeMulti => this.range_song_multi(scope, index),
                    SongRowClickAction::Select => this.select_song_row(scope, index),
                    SongRowClickAction::Play => {
                        this.select_song_row(scope, index);
                        this.play_song_at_scope(scope, index);
                    }
                }
                cx.notify();
            }),
        )
        .context_menu(move |menu, _, cx| {
            menu_app
                .update(cx, |this, cx| {
                    let context = this.song_menu_context(scope, index, editable);
                    let Some(context) = context else {
                        return menu;
                    };
                    this.song_menu(menu, context, cx)
                })
                .unwrap_or_else(|_| unreachable!("app outlives its rows"))
        })
        .into_any_element()
    }

    /// 打开歌曲菜单时从当前状态构建菜单上下文；列表已变化时返回 None。
    pub(super) fn song_menu_context(
        &self,
        scope: &str,
        index: usize,
        editable: bool,
    ) -> Option<SongMenuContext> {
        let songs = self.songs_for_scope(scope)?;
        if index >= songs.len() {
            return None;
        }
        Some(SongMenuContext {
            songs: Arc::new(songs.to_vec()),
            index,
            queue: editable,
            playlist: (scope == "playlist-detail")
                .then(|| self.state.current_playlist.clone())
                .flatten(),
        })
    }
    /// 服务器设置卡：上半部分是服务器列表（点击切换，行尾铅笔/垃圾桶
    /// 分别编辑与删除那一行），下半部分是编辑表单——仅在铅笔点开某行
    /// 或该条目尚未配置（首次运行/刚新增）时存在，平时不铺任何表单。
    /// 编辑不切换服务器：改另一台的地址账号不会动当前正在播的库；
    /// 断线重试在状态行上，不需要先进表单绕一圈。
    pub(super) fn render_server_status_card(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let connected = self.state.server.is_some();
        let active_configured = self
            .config
            .active_profile()
            .is_some_and(|profile| profile.is_configured());
        let status = if self.state.loading {
            t!("Connecting…").to_string()
        } else if connected {
            match self.state.server.as_ref() {
                Some(server) => t!(
                    "Connected · %{app} %{version}",
                    app = server.app_name.as_deref().unwrap_or("Navidrome"),
                    version = server.version
                )
                .to_string(),
                None => t!("Connected").to_string(),
            }
        } else if !active_configured {
            // 未配置的条目表单直接展开，提示语指向表单。
            t!("Not connected — fill in the address and account").to_string()
        } else if self.config.servers.len() > 1 {
            // 列表就在状态行上方，措辞指向列表而不是方位。
            t!("Not connected — pick a server in the list").to_string()
        } else {
            t!("Not connected").to_string()
        };
        // accent 是背景 token（亮色 neutral-100、深色 neutral-800），做前景
        // 会与卡片底色同值；连接态的强调色用 primary。
        let status_color = if connected {
            cx.theme().primary
        } else {
            cx.theme().muted_foreground
        };
        // 表单目标：行内铅笔打开的条目；没有显式目标时，尚未配置的
        // 激活条目（首次运行、刚新增）也直接展示表单——那时表单就是
        // 主任务。已配置的服务器默认不铺任何表单，编辑入口在每行里。
        let form_target = self
            .state
            .editing_server
            .as_deref()
            .and_then(|id| self.config.servers.iter().find(|profile| profile.id == id))
            .or_else(|| {
                (!active_configured)
                    .then(|| self.config.active_profile())
                    .flatten()
            })
            .cloned();
        // 密码存储位置提示（里程碑 #17）：已迁入系统凭据库的条目不再
        // 往配置文件里写明文，用户需要在设置页能确认当前状态。
        let credential_note = if self
            .config
            .active_profile()
            .is_some_and(|profile| profile.password_in_keyring)
        {
            t!("Password is kept in the system credential store")
        } else {
            t!("Password is kept in the config file")
        };

        let mut card = v_flex()
            .w_full()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border.opacity(0.72))
            .bg(cx.theme().secondary.opacity(0.18))
            // ---- 上：服务器列表 ----
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .p_4()
                    .child(self.render_server_list(cx))
                    .child(
                        v_flex()
                            .gap_0p5()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(div().text_xs().text_color(status_color).child(status))
                                    // 断线时给一个就地重试：重连不必先改地址账号。
                                    .when(
                                        !connected && active_configured && !self.state.loading,
                                        |row| {
                                            row.child(
                                                Button::new("retry-server-connection")
                                                    .label(t!("Retry"))
                                                    .tooltip(t!("Reconnect to the selected server"))
                                                    .ghost()
                                                    .small()
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.reconnect_active_server();
                                                        cx.notify();
                                                    })),
                                            )
                                        },
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(credential_note),
                            ),
                    ),
            );
        // ---- 下：编辑表单（仅在行内铅笔点开或条目未配置时存在） ----
        if let Some(target) = &form_target {
            card = card
                .child(div().h(px(1.0)).bg(cx.theme().border))
                .child(self.render_server_edit_form(target, cx));
        }
        card.into_any_element()
    }

    /// 连接表单：行内铅笔点开某一行、或该条目尚未配置（首次运行 /
    /// 刚新增）时展示，标题写明正在编辑哪一台。保存按钮文案随目标
    /// 与改动内容变化：目标是当前连接的服务器且动了地址/账号/密码
    /// 才是“保存并连接”；编辑别的服务器或只改显示名就是普通“保存”。
    fn render_server_edit_form(
        &self,
        target: &ServerProfile,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let target_configured = target.is_configured();
        let is_active = self.config.active_server.as_deref() == Some(target.id.as_str());
        let url = self.server_input.read(cx).value().trim().to_string();
        let username = self.username_input.read(cx).value().trim().to_string();
        let password = self.password_input.read(cx).value().to_string();
        let identity_dirty = target.url.trim() != url
            || target.username.trim() != username
            || target.resolved_password() != password;
        v_flex()
            .w_full()
            .gap_3()
            .p_4()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(
                        if target_configured {
                            t!("Editing “%{editing}”", editing = target.label())
                        } else {
                            t!("Configure your Navidrome server")
                        },
                    ))
                    .when(self.config.servers.len() > 1, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("switch servers in the list above")),
                        )
                    }),
            )
            .child(self.render_server_field(t!("Display name"), &self.name_input, cx))
            .child(self.render_server_field(t!("Server URL"), &self.server_input, cx))
            .child(self.render_server_field(t!("Username"), &self.username_input, cx))
            .child(self.render_server_field(t!("Password"), &self.password_input, cx))
            // 连接状态只与当前连接的服务器有关：编辑另一台时不显示，
            // 避把 A 的连接错误误读成 B 的问题。
            .when(is_active, |form| {
                form.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(self.state.status.clone()),
                )
            })
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    // 未配置的条目没有可取消回去的东西，不给退路按钮。
                    .when(target_configured, |row| {
                        row.child(
                            Button::new("cancel-server-edit")
                                .label(t!("Cancel"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.cancel_server_edit(window, cx);
                                })),
                        )
                    })
                    .child(
                        Button::new("save-settings")
                            .label(if is_active && (identity_dirty || !target_configured) {
                                // 当前连接的服务器且改了连接身份：保存后
                                // 断开重连；未配置条目的保存意图也是
                                // 建立连接。
                                t!("Save and connect")
                            } else {
                                t!("Save")
                            })
                            .primary()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.save_settings(cx);
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    /// 一行「标签 + 输入框」的紧凑表单行：标签定宽左对齐，
    /// 保证四个字段的输入框起始位置完全一致。
    fn render_server_field(
        &self,
        label: impl Into<SharedString>,
        input: &Entity<InputState>,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let label: SharedString = label.into();
        h_flex()
            .w_full()
            .gap_3()
            .items_center()
            .child(
                div()
                    .w(px(110.0))
                    .flex_none()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label),
            )
            .child(div().flex_1().min_w_0().child(Input::new(input).w_full()))
            .into_any_element()
    }

    /// 已保存服务器列表：点击切换，右侧按钮删除条目。
    /// 这是多服务器的主要入口，排在编辑表单之前。
    /// 服务器列表：一行一台，整行可点即切换（比在行尾找小按钮快），
    /// 只有删除按钮留在行尾。选中项用左侧实心点 + 高亮底色表示。
    fn render_server_list(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let active = self.config.active_server.clone();
        let can_remove = self.config.servers.len() > 1;
        v_flex()
            .w_full()
            .gap_1()
            .children(self.config.servers.iter().map(|profile| {
                let switch_id = profile.id.clone();
                let remove_id = profile.id.clone();
                let edit_id = profile.id.clone();
                let is_active = active.as_deref() == Some(profile.id.as_str());
                let label = profile.label();
                let detail = if profile.is_configured() {
                    t!(
                        "%{url} · %{user}",
                        url = profile.url,
                        user = profile.username
                    )
                    .to_string()
                } else {
                    t!("Not configured yet").to_string()
                };
                let switchable = !is_active && profile.is_configured();
                let tooltip = if is_active {
                    t!("Currently connected").to_string()
                } else if switchable {
                    t!(
                        "Switch to %{label}\n%{detail}",
                        label = label,
                        detail = detail
                    )
                    .to_string()
                } else {
                    t!("%{label} is not configured yet", label = label).to_string()
                };
                h_flex()
                    .id(SharedString::from(format!("server-row-{switch_id}")))
                    .w_full()
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(if is_active {
                        // 选中态的边框用框架的 list_active_border（亮色 blue-400、
                        // 深色 blue-700），与行底同一个蓝色族，不再是灰上加灰。
                        cx.theme().list_active_border.opacity(0.55)
                    } else {
                        cx.theme().border.opacity(0.7)
                    })
                    // 选中行底色沿用框架的选中语义色 list_active——accent 是
                    // 背景 token，0.07 叠在卡片底上等于没叠（与歌曲行同一教训）。
                    .bg(if is_active {
                        cx.theme().list_active
                    } else {
                        cx.theme().background
                    })
                    .when(switchable, |this| {
                        // hover 预演选中态：从白色走向同一片蓝色，
                        // 而不是把鼠标经过变成一次无意义的灰色闪光。
                        this.cursor_pointer()
                            .hover(|style| style.bg(cx.theme().list_active))
                    })
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                    // 当前连接由行底、图标与行尾胶囊三重表达，
                    // 原来那个 6px 圆点在这里只是噪声，已移除。
                    .child(
                        Icon::new(if is_active {
                            AppIcon::Server
                        } else {
                            AppIcon::ServerOff
                        })
                        .with_size(px(14.0))
                        // 图标绝不能再用 accent：它是背景 token，亮色主题下
                        // ≈#f5f5f5，与选中行底只差 1/255，看起来就是“图标不显示”。
                        .text_color(if is_active {
                            cx.theme().primary
                        } else {
                            cx.theme().muted_foreground
                        }),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(if is_active {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::MEDIUM
                                    })
                                    .truncate()
                                    .child(label),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(detail),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .gap_1()
                            .items_center()
                            .when(is_active, |this| {
                                this.child(
                                    // 胶囊：实心主色 + 主色前景，而不是半透明灰底
                                    // 靠 4px 小圆角硬拗的“标签”。用 primary /
                                    // primary_foreground 一对 token，亮暗主题都
                                    // 自动拿到足够对比度（≈17:1）。
                                    div()
                                        .flex_none()
                                        .flex()
                                        .items_center()
                                        .h(px(20.0))
                                        .px(px(8.0))
                                        .rounded(px(999.0))
                                        .bg(cx.theme().primary)
                                        .text_color(cx.theme().primary_foreground)
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(t!("Active")),
                                )
                            })
                            .child(
                                Button::new(SharedString::from(format!("edit-server-{edit_id}")))
                                    .icon(AppIcon::Pencil)
                                    .tooltip(t!("Edit connection"))
                                    .ghost()
                                    .small()
                                    // 行本身可点击切换：行内按钮必须拦住鼠标按下，
                                    // 否则会“打开编辑表单”与“切换服务器”同时发生。
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.begin_edit_server(&edit_id, window, cx);
                                    })),
                            )
                            .child(
                                Button::new(SharedString::from(format!(
                                    "remove-server-{remove_id}"
                                )))
                                .icon(AppIcon::Trash)
                                .tooltip(if can_remove {
                                    t!("Remove this server")
                                } else {
                                    t!("Keep at least one server")
                                })
                                .disabled(!can_remove)
                                .ghost()
                                .small()
                                // 同上：可切换行里的删除按钮此前会连带触发切换
                                // （先弹删除确认、随后又切到那台），拦住冒泡。
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        this.confirm_remove_server(&remove_id, window, cx);
                                        cx.notify();
                                    },
                                )),
                            ),
                    )
                    .when(switchable, |this| {
                        this.on_click(cx.listener(move |this, _, window, cx| {
                            this.switch_server(&switch_id, window, cx);
                            cx.notify();
                        }))
                    })
            }))
            .into_any_element()
    }

    /// 删除服务器的确认弹窗：两个递进选项——只移除条目，或连同清理
    /// 这台服务器的本地数据（播放会话/响度/流派缓存与已存凭据）。
    pub(super) fn render_remove_server_dialog(
        &self,
        dialog: gpui_kit::component::dialog::Dialog,
        cx: &Context<Self>,
    ) -> gpui_kit::component::dialog::Dialog {
        let Some((_, label)) = &self.server_remove_prompt else {
            return dialog;
        };
        let footer = h_flex()
            .justify_end()
            .gap_2()
            .child(
                Button::new("cancel-remove-server")
                    .label(t!("Cancel"))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.cancel_remove_server(cx);
                        window.close_dialog(cx);
                    })),
            )
            .child(
                Button::new("confirm-remove-server")
                    .label(t!("Remove"))
                    .danger()
                    .outline()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.submit_remove_server(false, window, cx);
                        window.close_dialog(cx);
                    })),
            )
            .child(
                Button::new("confirm-remove-server-data")
                    .label(t!("Remove and clear data"))
                    .danger()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.submit_remove_server(true, window, cx);
                        window.close_dialog(cx);
                    })),
            );
        let ok_app = cx.entity().downgrade();
        dialog
            .title(t!("Remove server?"))
            .w(px(480.0))
            .overlay_closable(false)
            .keyboard(true)
            // Enter 芓当于“只移除”，保守默认不清理本地数据。
            .on_ok(move |_, window, cx| {
                let _ = ok_app.update(cx, |this, cx| {
                    this.submit_remove_server(false, window, cx)
                });
                true
            })
            .on_close(cx.listener(|this, _, _, cx| {
                this.cancel_remove_server(cx);
            }))
            .child(
                v_flex()
                    .gap_2()
                    .p_1()
                    .child(div().text_sm().child(t!("Remove “%{label}” from this app? The server itself and its music stay untouched.",
                        label = label
                    )))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("Remove keeps local data: the saved session, loudness and genre caches, and the stored credential are kept, so re-adding the same server continues where you left off. Remove and clear data deletes them for good — the password will have to be re-entered."))
                                ),
                    ),
            )
            .footer(footer)
    }
    /// 设置覆盖层：盖住侧边栏与内容区，点击背景或 Esc 关闭；
    /// 标题栏和底部播放条不被遮挡，改设置时也能控制播放。
    pub(super) fn render_settings_overlay(
        &self,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let settings_content = div()
            .id("settings-content")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.settings_scroll)
            .vertical_scrollbar(&self.settings_scroll)
            .when(compact, |this| this.px_5().py_4())
            .when(!compact, |this| this.px_6().py_5())
            .child(self.render_settings(cx));
        let settings_body = if compact {
            v_flex()
                .id("settings-body")
                .flex_1()
                .min_h_0()
                .items_stretch()
                .child(self.render_settings_navigation(true, cx))
                .child(settings_content)
                .into_any_element()
        } else {
            h_flex()
                .id("settings-body")
                .flex_1()
                .min_h_0()
                .items_stretch()
                .child(self.render_settings_navigation(false, cx))
                .child(settings_content)
                .into_any_element()
        };

        div()
            .id("settings-backdrop")
            .absolute()
            .inset_0()
            .occlude() // 遮罩挡住背后所有命中框：后方页面不再响应滚轮/悬停
            .flex()
            .justify_center()
            .items_start()
            // 固定黑色半透明背景：亮/暗主题下都是压暗效果。
            .bg(hsla(0.0, 0.0, 0.0, 0.35))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.state.settings_open = false;
                    this.cancel_shortcut_recording();
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .id("settings-card")
                    .w(px(960.0))
                    .h(px(650.0))
                    .max_w(relative(if compact { 0.96 } else { 0.94 }))
                    .max_h(relative(if compact { 0.94 } else { 0.88 }))
                    .mt(px(if compact { 16.0 } else { 48.0 }))
                    .bg(cx.theme().background)
                    .rounded_xl()
                    .shadow_xl()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        // 点击卡片内部不触发背景关闭。
                        cx.stop_propagation()
                    })
                    .child(
                        h_flex()
                            .flex_none()
                            .when(compact, |this| this.px_4().py_2())
                            .when(!compact, |this| this.px_5().py_3())
                            .justify_between()
                            .items_center()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("Settings")),
                            )
                            .child(
                                Button::new("close-settings")
                                    .icon(AppIcon::Close)
                                    .tooltip(t!("Close settings"))
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.state.settings_open = false;
                                        this.cancel_shortcut_recording();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(settings_body),
            )
            .into_any_element()
    }

    fn settings_navigation_button(
        &self,
        id: &'static str,
        section: SettingsSection,
        label: SharedString,
        icon: AppIcon,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let selected = self.state.settings_section == section;
        let button = Button::new(id)
            .w_full()
            .h(px(36.0))
            .when(!compact, |this| this.justify_start())
            .accessibility_label(label.clone())
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .when(!compact, |this| this.justify_start())
                    .child(
                        div()
                            .w(px(20.0))
                            .flex_none()
                            .flex()
                            .justify_center()
                            .child(Icon::new(icon).with_size(px(16.0))),
                    )
                    .child(div().min_w_0().text_sm().truncate().child(label)),
            )
            .ghost()
            .selected(selected)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.cancel_shortcut_recording();
                this.state.settings_section = section;
                this.settings_scroll.scroll_to_top_of_item(0);
                cx.notify();
            }));

        if compact {
            div().flex_1().min_w_0().child(button).into_any_element()
        } else {
            button.into_any_element()
        }
    }

    fn render_settings_navigation(
        &self,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let buttons = vec![
            self.settings_navigation_button(
                "settings-nav-servers",
                SettingsSection::Servers,
                t!("Servers").into(),
                AppIcon::Server,
                compact,
                cx,
            ),
            self.settings_navigation_button(
                "settings-nav-playback",
                SettingsSection::Playback,
                t!("Playback").into(),
                AppIcon::VolumeHigh,
                compact,
                cx,
            ),
            self.settings_navigation_button(
                "settings-nav-desktop",
                SettingsSection::Desktop,
                t!("Desktop").into(),
                AppIcon::MiniPlayer,
                compact,
                cx,
            ),
            self.settings_navigation_button(
                "settings-nav-appearance",
                SettingsSection::Appearance,
                t!("Appearance").into(),
                AppIcon::Settings,
                compact,
                cx,
            ),
            self.settings_navigation_button(
                "settings-nav-shortcuts",
                SettingsSection::Shortcuts,
                t!("Shortcuts").into(),
                AppIcon::Keyboard,
                compact,
                cx,
            ),
        ];

        if compact {
            h_flex()
                .w_full()
                .flex_none()
                .gap_1()
                .p_2()
                .border_b_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary.opacity(0.25))
                .children(buttons)
                .into_any_element()
        } else {
            v_flex()
                .w(px(184.0))
                .flex_none()
                .gap_1()
                .p_3()
                .border_r_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().secondary.opacity(0.25))
                .children(buttons)
                .into_any_element()
        }
    }

    pub(super) fn render_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let cache_dir = config::audio_cache_dir(&self.config);
        let (title, description) = match self.state.settings_section {
            SettingsSection::Servers => (
                t!("Servers"),
                t!("Manage server connections and library access."),
            ),
            SettingsSection::Playback => (
                t!("Playback"),
                t!("Configure streaming, audio output, and listening preferences."),
            ),
            SettingsSection::Desktop => (
                t!("Desktop"),
                t!("Control notifications and window behavior."),
            ),
            SettingsSection::Appearance => (
                t!("Appearance"),
                t!("Choose the theme and interface language."),
            ),
            SettingsSection::Shortcuts => (
                t!("Shortcuts"),
                t!("Customize keyboard shortcuts for playback and navigation."),
            ),
        };
        v_flex()
            .w_full()
            .max_w(px(840.0))
            .gap_5()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(description),
                    ),
            )
            .when(self.state.settings_section == SettingsSection::Servers, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(t!("Connections")),
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .when(self.api.is_some(), |this| {
                                        this.child(
                                            Button::new("settings-refresh")
                                                .icon(AppIcon::Refresh)
                                                .tooltip(t!("Refresh library"))
                                                .loading(self.state.loading)
                                                .ghost()
                                                .small()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.refresh_library();
                                                    cx.notify();
                                                })),
                                        )
                                    })
                                    .child(
                                        Button::new("settings-add-server")
                                            .icon(KitIcon::Plus)
                                            .label(t!("Add server"))
                                            .tooltip(t!("Add another server"))
                                            .ghost()
                                            .small()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.add_server(window, cx);
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    )
                    .child(self.render_server_status_card(cx)),
            ))
            .when(self.state.settings_section == SettingsSection::Playback, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                                    .child(t!("Streaming & cache")),
                    )
                    .child(
                        v_flex()
                            .gap_4()
                            .p_4()
                            .border_t_1()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.72))
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(t!("Audio cache folder")),
                                    )
                                    .child(
                                        h_flex()
                                            .w_full()
                                            .min_w_0()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .truncate()
                                                    .text_sm()
                                                    .child(cache_dir.display().to_string()),
                                            )
                                            .child(
                                                Button::new("choose-cache-directory")
                                                    .label(t!("Choose folder"))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.choose_cache_directory(cx);
                                                    })),
                                            )
                                            .child(
                                                Button::new("reset-cache-directory")
                                                    .label(t!("Use default"))
                                                    .disabled(self.config.cache_dir.is_none())
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.apply_cache_directory(None);
                                                        cx.notify();
                                                    })),
                                            ),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .gap_4()
                                    .child(
                                        div()
                                            .text_sm()
                                            .child(t!("Streaming quality")),
                                    )
                                    .child({
                                        let app = cx.entity().downgrade();
                                        Button::new("streaming-quality-select")
                                            .w(px(220.0))
                                            .justify_between()
                                            .label(self.config.transcoding_quality.label())
                                            .dropdown_caret(true)
                                            .outline()
                                            .dropdown_menu(move |menu, _, cx| {
                                                app.update(cx, |this, cx| {
                                                    let mut menu = menu;
                                                    for quality in TranscodingQuality::ALL {
                                                        menu = menu.item(
                                                            PopupMenuItem::new(quality.label())
                                                                .checked(
                                                                    this.config
                                                                        .transcoding_quality
                                                                        == quality,
                                                                )
                                                                .on_click(cx.listener(
                                                                    move |this, _, _, cx| {
                                                                        this.set_transcoding_quality(
                                                                            quality, cx,
                                                                        );
                                                                    },
                                                                )),
                                                        );
                                                    }
                                                    menu
                                                })
                                                .expect("app owns streaming quality menu")
                                            })
                                    }),
                            ),
                    ),
            ))
            .when(self.state.settings_section == SettingsSection::Playback, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(t!("Audio output")),
                    )
                    .child(
                        v_flex()
                            .gap_4()
                            .p_4()
                            .border_t_1()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.72))
                            .child({
                                let playback = self.audio.state();
                                let active = match playback.output_name.as_deref() {
                                    Some(name) => name.to_string(),
                                    None => t!("System default").to_string(),
                                };
                                div().text_xs().text_color(cx.theme().muted_foreground).child(
                                    t!("Active: %{active} · %{rate} Hz · %{channels} ch",
                                        active = active,
                                        rate = playback.output_sample_rate,
                                        channels = playback.output_channels
                                    ),
                                )
                            })
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .flex_wrap()
                                    .child({
                                        let app = cx.entity().downgrade();
                                        let selected_label = self
                                            .config
                                            .output_device
                                            .clone()
                                            .unwrap_or_else(|| t!("System default").to_string());
                                        Button::new("output-device-select")
                                            .w(px(300.0))
                                            .max_w_full()
                                            .justify_between()
                                            .label(selected_label.clone())
                                            .tooltip(selected_label)
                                            .dropdown_caret(true)
                                            .outline()
                                            .dropdown_menu(move |menu, _, cx| {
                                                app.update(cx, |this, cx| {
                                                    let mut menu = menu.item(
                                                        PopupMenuItem::new(t!("System default"))
                                                            .checked(
                                                                this.config.output_device.is_none(),
                                                            )
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    this.set_output_device(None, cx);
                                                                },
                                                            )),
                                                    );
                                                    for (name, is_default) in
                                                        &this.state.output_devices
                                                    {
                                                        let name_for_click = name.clone();
                                                        let label = if *is_default {
                                                            t!(
                                                                "%{name} (default)",
                                                                name = name
                                                            )
                                                        } else {
                                                            t!("%{name}", name = name)
                                                        };
                                                        menu = menu.item(
                                                            PopupMenuItem::new(label)
                                                                .checked(
                                                                    this.config
                                                                        .output_device
                                                                        .as_deref()
                                                                        == Some(name.as_str()),
                                                                )
                                                                .on_click(cx.listener(
                                                                    move |this, _, _, cx| {
                                                                        this.set_output_device(
                                                                            Some(
                                                                                name_for_click
                                                                                    .clone(),
                                                                            ),
                                                                            cx,
                                                                        );
                                                                    },
                                                                )),
                                                        );
                                                    }
                                                    menu
                                                })
                                                .expect("app owns output device menu")
                                            })
                                    })
                                    .child(
                                        Button::new("refresh-output-devices")
                                            .icon(AppIcon::Refresh)
                                            .tooltip(t!("Refresh devices"))
                                            .ghost()
                                            .small()
                                            .loading(self.state.output_devices_loading)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.refresh_output_devices(cx);
                                            })),
                                    ),
                            ),
                    ),
            ))
            .when(self.state.settings_section == SettingsSection::Playback, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(t!("Volume normalization")),
                    )
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_4()
                            .p_4()
                            .border_t_1()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.72))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(
                                                t!("Adjust per-song loudness with ReplayGain so quiet and loud tracks play at a similar level."
                                                ),
                                            ),
                                    ),
                            )
                            .child({
                                let app = cx.entity().downgrade();
                                Button::new("volume-normalization-select")
                                    .w(px(220.0))
                                    .justify_between()
                                    .label(self.config.volume_normalization.label())
                                    .tooltip(self.config.volume_normalization.tooltip())
                                    .dropdown_caret(true)
                                    .outline()
                                    .dropdown_menu(move |menu, _, cx| {
                                        app.update(cx, |this, cx| {
                                            let mut menu = menu;
                                            for mode in VolumeNormalization::ALL {
                                                menu = menu.item(
                                                    PopupMenuItem::new(mode.label())
                                                        .checked(
                                                            this.config.volume_normalization
                                                                == mode,
                                                        )
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.set_volume_normalization(
                                                                    mode, cx,
                                                                );
                                                            },
                                                        )),
                                                );
                                            }
                                            menu
                                        })
                                        .expect("app owns volume normalization menu")
                                    })
                            }),
                    ),
            ))
            .when(self.state.settings_section == SettingsSection::Playback, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(t!("Lyrics")),
                    )
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_4()
                            .p_4()
                            .border_t_1()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.72))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .child(t!("Show translation")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(
                                                t!("Show translated lyrics under the original line when the server provides another language variant."
                                                ),
                                            ),
                                    ),
                            )
                            .child(
                                Switch::new("lyrics-translation-switch")
                                    .checked(self.config.show_lyrics_translation)
                                    .accessibility_label(t!("Show translation"))
                                    .on_change(cx.listener(|this, checked, _, cx| {
                                        this.set_show_lyrics_translation(*checked, cx);
                                    })),
                            ),
                    ),
            ))
            .when(self.state.settings_section == SettingsSection::Desktop, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                                    .child(t!("System integration")),
                    )
                    .child(
                        v_flex()
                            .gap_5()
                            .p_4()
                            .border_t_1()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.72))
                            .child(
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .gap_4()
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .child(t!("Now-playing notifications")),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(
                                                        t!("Send a desktop notification with the cover, title, artist and album when a track starts. Rapid switching replaces the previous notification."
                                                        ),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        Switch::new("track-notifications-switch")
                                            .checked(self.config.show_track_notifications)
                                            .accessibility_label(t!("Now-playing notifications"))
                                            .on_change(cx.listener(|this, checked, _, cx| {
                                                this.set_show_track_notifications(*checked, cx);
                                            })),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .justify_between()
                                    .items_center()
                                    .gap_4()
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w_0()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .child(t!("Close button minimizes")),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(cx.theme().muted_foreground)
                                                    .child(
                                                        t!("The window close button minimizes to the taskbar instead of quitting; tray Exit still quits. Some Wayland compositors ignore minimize requests."
                                                        ),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        Switch::new("close-minimizes-switch")
                                            .checked(self.config.close_minimizes)
                                            .accessibility_label(t!("Close button minimizes"))
                                            .on_change(cx.listener(|this, checked, _, cx| {
                                                this.set_close_minimizes(*checked, cx);
                                            })),
                                    ),
                            ),
                    ),
            ))
            .when(self.state.settings_section == SettingsSection::Appearance, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                                    .child(t!("Theme")),
                    )
                    .child(
                        v_flex()
                            .gap_3()
                            .p_4()
                            .border_t_1()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.72))
                            .child(
                                h_flex().gap_2().children(
                                    [
                                        ThemePreference::Light,
                                        ThemePreference::Dark,
                                        ThemePreference::System,
                                    ]
                                    .into_iter()
                                    .map(|preference| {
                                        // 同上：ID 用 Debug 名（Light/Dark/System），
                                        // 不随界面语言变化。
                                        Button::new(SharedString::from(format!(
                                            "theme-{preference:?}"
                                        )))
                                        .label(preference.label())
                                        .selected(self.config.theme == preference)
                                        .on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                this.set_theme(preference, window, cx);
                                            }),
                                        )
                                    }),
                                ),
                            ),
                    ),
            ))
            .when(self.state.settings_section == SettingsSection::Appearance, |settings| settings.child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child(t!("Language")),
                    )
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_4()
                            .p_4()
                            .border_t_1()
                            .border_b_1()
                            .border_color(cx.theme().border.opacity(0.72))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(t!(
                                                "Interface language. Changes apply instantly."
                                            )),
                                    ),
                            )
                            .child({
                                let app = cx.entity().downgrade();
                                Button::new("language-select")
                                    .w(px(220.0))
                                    .justify_between()
                                    .label(self.config.language.native_label())
                                    .dropdown_caret(true)
                                    .outline()
                                    .dropdown_menu(move |menu, _, cx| {
                                        app.update(cx, |this, cx| {
                                            let mut menu = menu;
                                            for preference in LanguagePreference::ALL {
                                                menu = menu.item(
                                                    PopupMenuItem::new(preference.native_label())
                                                        .checked(
                                                            this.config.language == preference,
                                                        )
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.set_language(preference, cx);
                                                            },
                                                        )),
                                                );
                                            }
                                            menu
                                        })
                                        .expect("app owns language menu")
                                    })
                            }),
                    ),
            ))
            .when(
                self.state.settings_section == SettingsSection::Shortcuts,
                |settings| settings.child(self.render_shortcut_settings(cx)),
            )
            .into_any_element()
    }

    fn render_shortcut_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        const PLAYBACK: &[ShortcutAction] = &[
            ShortcutAction::TogglePlayback,
            ShortcutAction::Previous,
            ShortcutAction::Next,
            ShortcutAction::SeekBack,
            ShortcutAction::SeekForward,
            ShortcutAction::VolumeUp,
            ShortcutAction::VolumeDown,
            ShortcutAction::ToggleMute,
        ];
        const NAVIGATION: &[ShortcutAction] = &[
            ShortcutAction::ToggleNowPlaying,
            ShortcutAction::ToggleQueue,
            ShortcutAction::FocusSearch,
            ShortcutAction::CloseOverlays,
        ];
        const SONG_LIST: &[ShortcutAction] = &[
            ShortcutAction::SelectPrevious,
            ShortcutAction::SelectNext,
            ShortcutAction::SelectAll,
            ShortcutAction::PlaySelected,
        ];
        let all_default = ShortcutAction::ALL
            .into_iter()
            .all(|action| self.config.shortcuts.binding(action) == action.default_binding());
        let all_global_default = ShortcutAction::GLOBAL_PLAYBACK.into_iter().all(|action| {
            self.config.global_shortcuts.binding(action)
                == action
                    .default_global_binding()
                    .expect("global shortcut actions have defaults")
        });

        v_flex()
            .gap_6()
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_4()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(t!("App shortcuts")),
                            )
                            .child(
                                Button::new("reset-all-shortcuts")
                                    .label(t!("Restore app defaults"))
                                    .ghost()
                                    .small()
                                    .disabled(all_default)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.reset_all_shortcuts(cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!(
                                "These shortcuts work while the app window is active. Click a shortcut, then press a new combination; press Esc to cancel."
                            )),
                    )
                    .child(self.render_shortcut_group(t!("Playback controls"), PLAYBACK, cx))
                    .child(self.render_shortcut_group(t!("Navigation"), NAVIGATION, cx))
                    .child(self.render_shortcut_group(t!("Song list"), SONG_LIST, cx)),
            )
            .child(
                v_flex()
                    .gap_3()
                    .pt_5()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_4()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(t!("Global shortcuts")),
                            )
                            .child(
                                Button::new("reset-all-global-shortcuts")
                                    .label(t!("Restore global defaults"))
                                    .ghost()
                                    .small()
                                    .disabled(!cfg!(target_os = "windows") || all_global_default)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.reset_all_global_shortcuts(cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if cfg!(target_os = "windows") {
                                t!("Control playback while the app is hidden or another window is active. Use the Windows key or at least two modifier keys.")
                            } else {
                                t!("Custom global shortcuts are currently available on Windows.")
                            }),
                    )
                    .child(self.render_global_shortcut_group(cx)),
            )
            .into_any_element()
    }

    fn render_shortcut_group(
        &self,
        title: impl Into<SharedString>,
        actions: &'static [ShortcutAction],
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        v_flex()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(title.into()),
            )
            .child(
                v_flex()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border.opacity(0.72))
                    .overflow_hidden()
                    .children(
                        actions.iter().enumerate().map(|(index, action)| {
                            self.render_shortcut_row(*action, index > 0, cx)
                        }),
                    ),
            )
            .into_any_element()
    }

    fn render_shortcut_row(
        &self,
        action: ShortcutAction,
        separated: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let recording = self.state.recording_shortcut == Some(action);
        let binding = self.config.shortcuts.binding(action);
        let is_default = binding == action.default_binding();
        let conflict = self
            .state
            .shortcut_conflict
            .filter(|(target, _)| *target == action)
            .map(|(_, existing)| existing);

        v_flex()
            .px_4()
            .py_3()
            .gap_1()
            .when(separated, |row| {
                row.border_t_1().border_color(cx.theme().border)
            })
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .child(shortcut_label(action)),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                Button::new(SharedString::from(format!(
                                    "shortcut-binding-{action:?}"
                                )))
                                .w(px(190.0))
                                .label(if recording {
                                    t!("Press shortcut…").to_string()
                                } else {
                                    shortcut_display(binding)
                                })
                                .outline()
                                .selected(recording)
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        if this.state.recording_shortcut == Some(action) {
                                            this.cancel_shortcut_recording();
                                        } else {
                                            this.start_shortcut_recording(action, false);
                                        }
                                        window.blur(cx);
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                Button::new(SharedString::from(format!(
                                    "shortcut-reset-{action:?}"
                                )))
                                .icon(KitIcon::Undo2)
                                .tooltip(t!("Restore default"))
                                .ghost()
                                .small()
                                .disabled(is_default)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.reset_shortcut(action, cx);
                                    },
                                )),
                            ),
                    ),
            )
            .when_some(conflict, |row, existing| {
                row.child(div().text_xs().text_color(cx.theme().danger).child(t!(
                    "Already assigned to %{action}",
                    action = shortcut_label(existing)
                )))
            })
            .into_any_element()
    }

    fn render_global_shortcut_group(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border.opacity(0.72))
            .overflow_hidden()
            .children(
                ShortcutAction::GLOBAL_PLAYBACK
                    .iter()
                    .enumerate()
                    .map(|(index, action)| self.render_global_shortcut_row(*action, index > 0, cx)),
            )
            .into_any_element()
    }

    fn render_global_shortcut_row(
        &self,
        action: ShortcutAction,
        separated: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let recording = self.state.recording_global_shortcut == Some(action);
        let binding = self.config.global_shortcuts.binding(action);
        let is_default = binding
            == action
                .default_global_binding()
                .expect("global shortcut actions have defaults");
        let conflict = self
            .state
            .global_shortcut_conflict
            .filter(|(target, _)| *target == action)
            .map(|(_, existing)| existing);
        let validation = self
            .state
            .global_shortcut_validation
            .filter(|(target, _)| *target == action)
            .map(|(_, error)| error);
        let registration_failed = self.state.global_shortcut_failures.contains(&action);

        v_flex()
            .px_4()
            .py_3()
            .gap_1()
            .when(separated, |row| {
                row.border_t_1().border_color(cx.theme().border)
            })
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .child(shortcut_label(action)),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                Button::new(SharedString::from(format!(
                                    "global-shortcut-binding-{action:?}"
                                )))
                                .w(px(190.0))
                                .label(if recording {
                                    t!("Press shortcut…").to_string()
                                } else {
                                    shortcut_display(binding)
                                })
                                .outline()
                                .selected(recording)
                                .disabled(!cfg!(target_os = "windows"))
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        if this.state.recording_global_shortcut == Some(action) {
                                            this.cancel_shortcut_recording();
                                        } else {
                                            this.start_shortcut_recording(action, true);
                                        }
                                        window.blur(cx);
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                Button::new(SharedString::from(format!(
                                    "global-shortcut-reset-{action:?}"
                                )))
                                .icon(KitIcon::Undo2)
                                .tooltip(t!("Restore default"))
                                .ghost()
                                .small()
                                .disabled(!cfg!(target_os = "windows") || is_default)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.reset_global_shortcut(action, cx);
                                    },
                                )),
                            ),
                    ),
            )
            .when_some(conflict, |row, existing| {
                row.child(div().text_xs().text_color(cx.theme().danger).child(t!(
                    "Already assigned to %{action}",
                    action = shortcut_label(existing)
                )))
            })
            .when_some(validation, |row, error| {
                row.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(match error {
                            GlobalShortcutValidationError::UnsafeModifiers => {
                                t!("Use the Windows key or at least two modifier keys.")
                            }
                            GlobalShortcutValidationError::UnsupportedKey => {
                                t!("That key cannot be used as a global shortcut.")
                            }
                        }),
                )
            })
            .when(registration_failed, |row| {
                row.child(div().text_xs().text_color(cx.theme().danger).child(t!(
                    "Could not register this shortcut. It may already be used by another app."
                )))
            })
            .into_any_element()
    }

    /// Toast 通知堆叠：绝对定位在内容区右下角，覆盖在页面之上。
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
                            }),
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
                    .rounded_lg()
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
                                    .text_sm()
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
                                    .text_color(cx.theme().muted_foreground)
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
                                    .child(
                                        Button::new("home-feature-playback")
                                            .label(if is_playing {
                                                t!("Pause")
                                            } else {
                                                t!("Play")
                                            })
                                            .icon(if is_playing {
                                                PlayerIcon::Pause
                                            } else {
                                                PlayerIcon::Play
                                            })
                                            .custom(play_style)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.toggle_playback();
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new("home-feature-now-playing")
                                            .label(t!("Now Playing"))
                                            .ghost()
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
                .rounded_lg()
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
                                .text_sm()
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
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(album.artist.clone()),
                        )
                        .child(
                            h_flex()
                                .when(compact, |this| this.pt_1().gap_1())
                                .when(!compact, |this| this.pt_2().gap_2())
                                .child(
                                    Button::new("home-feature-play-album")
                                        .label(t!("Play album"))
                                        .icon(PlayerIcon::Play)
                                        .custom(play_style)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.play_album(album_for_play.clone());
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    Button::new("home-feature-open-album")
                                        .label(t!("Go to album"))
                                        .ghost()
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

        v_flex()
            .gap_6()
            .child(self.page_header(t!("Home"), status, cx))
            .when_some(self.render_home_feature(window, cx), |this, feature| {
                this.child(feature)
            })
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

    pub(super) fn render_queue_drawer(
        &self,
        width: f32,
        overlay: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let accent = self.now_playing_accent(cx);
        let compact = width <= 340.0;
        let play_all_style = ButtonCustomVariant::new(cx)
            .color(accent)
            .foreground(accent_foreground(accent))
            .hover(adjust_lightness(accent, 0.05))
            .active(adjust_lightness(accent, -0.05));

        v_flex()
            .id("queue-drawer")
            .w(px(width))
            .min_w_0()
            .flex_none()
            .when(overlay, |this| this.absolute().top_0().right_0().bottom_0())
            .when(!overlay, |this| this.h_full())
            .occlude()
            .bg(cx.theme().background)
            .border_l_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .h(px(56.0))
                    .flex_none()
                    .px_4()
                    .items_center()
                    .justify_between()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        v_flex()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("Up next")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("%{count} tracks", count = self.state.queue.len())),
                            ),
                    )
                    .child(
                        Button::new("close-queue-drawer")
                            .icon(AppIcon::Close)
                            .tooltip(t!("Close"))
                            .ghost()
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.state.queue_drawer_open = false;
                                this.clear_song_multi();
                                cx.notify();
                            })),
                    ),
            )
            .when(self.state.queue.is_empty(), |this| {
                this.child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .px_6()
                        .gap_2()
                        .text_center()
                        .child(
                            Icon::new(AppIcon::Queue)
                                .with_size(px(28.0))
                                .text_color(cx.theme().muted_foreground),
                        )
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(t!("Queue is empty")),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("Start playing an album or playlist to build a queue.")),
                        ),
                )
            })
            .when(!self.state.queue.is_empty(), |this| {
                this.child(
                    h_flex()
                        .flex_none()
                        .when(compact, |this| this.px_2())
                        .when(!compact, |this| this.px_3())
                        .py_2()
                        .gap_0p5()
                        .bg(cx.theme().secondary.opacity(0.18))
                        .border_b_1()
                        .border_color(cx.theme().border.opacity(0.7))
                        .child(
                            Button::new("drawer-queue-play-all")
                                .icon(PlayerIcon::Play)
                                .label(t!("Play all"))
                                .tooltip(t!("Play all"))
                                .custom(play_all_style)
                                .small()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let songs = this.state.queue.clone();
                                    if !songs.is_empty() {
                                        this.play_song_list(&songs, 0);
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("drawer-queue-clear-after")
                                .label(if compact {
                                    t!("Clear after")
                                } else {
                                    t!("Clear after current")
                                })
                                .tooltip(t!("Clear after current"))
                                .ghost()
                                .small()
                                .disabled(self.state.queue_index.is_none())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.clear_queue_after_current();
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("drawer-save-queue-playlist")
                                .icon(AppIcon::Playlists)
                                .tooltip(t!("Save as playlist"))
                                .ghost()
                                .small()
                                .disabled(self.playlist_ui.busy || self.api.is_none())
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.name_playlist(
                                        None,
                                        crate::playlist::song_ids(&this.state.queue),
                                        window,
                                        cx,
                                    );
                                })),
                        )
                        .child(div().flex_1())
                        .child(
                            Button::new("drawer-queue-clear-all")
                                .icon(AppIcon::Trash)
                                .tooltip(t!("Clear queue"))
                                .ghost()
                                .small()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.clear_queue();
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_h_0()
                        .child(self.render_song_list("queue", true, cx)),
                )
            })
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
                    .border_1()
                    .border_color(cx.theme().border)
                    .rounded_lg()
                    .overflow_hidden()
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
                                        this.border_t_1().border_color(cx.theme().border)
                                    })
                                    .hover(|style| style.bg(cx.theme().accent.opacity(0.08)))
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
                                                    .font_weight(FontWeight::SEMIBOLD)
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
                    .child(self.page_header(
                        t!("Playlists"),
                        t!("%{count} playlists", count = self.state.playlists.len()),
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("new-playlist")
                                    .label(t!("New playlist"))
                                    .primary()
                                    .small()
                                    .disabled(self.playlist_ui.busy || self.api.is_none())
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
                            .child(
                                Button::new("refresh-playlists")
                                    .icon(AppIcon::Refresh)
                                    .label(t!("Refresh"))
                                    .outline()
                                    .small()
                                    .disabled(
                                        self.playlist_ui.busy || self.playlist_ui.list_loading,
                                    )
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.load_playlists();
                                        cx.notify();
                                    })),
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
                                div()
                                    .py_8()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("No playlists are available.")),
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
                v_flex()
                    .gap_5()
                    .child(self.page_header(
                        artist.name.clone(),
                        t!("%{count} albums", count = self.state.artist_albums.len()),
                        cx,
                    ))
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
                                            this.play_song_list(&songs, 0);
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
                                        this.append_all(&songs);
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

    pub(super) fn render_mini(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui_kit::AnyElement {
        let theme = cx.theme().clone();
        let song = self.state.now_playing.clone();
        let has_track = song.is_some();
        let playback = self.audio.state();
        let is_playing = playback.active && !playback.paused;
        let duration = playback.duration.unwrap_or_default();
        let progress_percent = if duration.is_zero() {
            0.0
        } else {
            (playback.position.as_secs_f32() / duration.as_secs_f32() * 100.0).clamp(0.0, 100.0)
        };

        let cover_id = song.as_ref().and_then(|song| song.cover_art.as_deref());
        let cover = cover_id
            .and_then(|id| self.state.covers.get(id))
            .cloned()
            .unwrap_or_else(|| self.default_cover.clone());
        self.mark_cover_image_used(&cover);

        let (base, raw_accent) = self.now_playing_palette(cx);
        let accent = self.now_playing_accent(cx);
        let accent_fg = accent_foreground(accent);
        let play_button_style = ButtonCustomVariant::new(cx)
            .color(accent)
            .foreground(accent_fg)
            .hover(adjust_lightness(accent, 0.05))
            .active(adjust_lightness(accent, -0.05))
            .shadow(true);

        let light = theme.background.l >= 0.5;
        let base_strength = if light { 0.36 } else { 0.3 };
        let accent_strength = if light { 0.28 } else { 0.24 };
        let bg_start = theme.background.blend(base.opacity(base_strength));
        let bg_end = theme.background.blend(raw_accent.opacity(accent_strength));
        let bg_animation = Animation::new(Duration::from_secs(20)).repeat();

        let title = song
            .as_ref()
            .map(|song| song.title.clone())
            .unwrap_or_else(|| t!("No track playing").to_string());
        let artist = song
            .as_ref()
            .map(|song| song.artist.clone())
            .unwrap_or_else(|| t!("Choose a song").to_string());

        let cover_element = img(cover)
            // `on_click` only fires for elements with an id: it needs element state
            // to track the press/release pair.
            .id("mini-cover")
            .size(px(34.0))
            .aspect_square()
            .object_fit(ObjectFit::Cover)
            .flex_none()
            .rounded_md()
            .cursor_pointer()
            // 播放中封面外发光随封面调色板同步，静止时保留一层柔和阴影。
            .when(is_playing, |this| {
                this.shadow(vec![box_shadow(
                    px(0.0),
                    px(0.0),
                    px(7.0),
                    px(0.0),
                    accent.opacity(0.5),
                )])
            })
            .when(!is_playing, |this| {
                this.shadow(vec![box_shadow(
                    px(0.0),
                    px(1.0),
                    px(3.0),
                    px(0.0),
                    hsla(0.0, 0.0, 0.0, 0.28),
                )])
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| {
                this.exit_mini_mode(window, cx);
                this.open_now_playing();
            }))
            .occlude();

        // Fixed widths in the mini row that leave room for the scrolling info text:
        // px_1p5 padding (12) + cover (34) + gap_1p5 between the 4 row children (18)
        // + controls (18+4+26+4+18 = 70) + expand button (18) = 152. On Linux the
        // window shadow shrinks the content, so use the content (inner) width.
        let paddings = window_paddings(window);
        let content_width = window.viewport_size().width - paddings.left - paddings.right;
        let info_width = f32::from(content_width - px(152.0)).max(1.0);
        let track_id = song
            .as_ref()
            .map(|song| song.id.clone())
            .unwrap_or_default();
        let title_id = SharedString::from(format!("mini-title-{track_id}"));
        let artist_id = SharedString::from(format!("mini-artist-{track_id}"));

        let info = v_flex()
            .flex_1()
            .min_w_0()
            .gap_0p5()
            .overflow_hidden()
            .id("mini-now-playing-info")
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| {
                this.exit_mini_mode(window, cx);
                this.open_now_playing();
            }))
            .occlude()
            .child(self.render_scrolling_text(
                title.clone().into(),
                info_width,
                rems(0.75).to_pixels(window.rem_size()),
                FontWeight::SEMIBOLD,
                title_id,
                window,
            ))
            .child(
                // 迷你窗口没有播放条，用灰字换掉艺人行来区分「在等网络」和
                // 「真的出错」，否则用户看到的只是界面静止。
                div()
                    .text_color(theme.muted_foreground)
                    .when(playback.stall.is_some(), |this| {
                        this.child(t!(
                            "Buffering… %{seconds}s",
                            seconds = playback.stall.unwrap_or_default().as_secs()
                        ))
                    })
                    .when(
                        playback.stall.is_none() && playback.error.is_some(),
                        |this| this.child(t!("Playback failed — open for details")),
                    )
                    .when(
                        playback.stall.is_none() && playback.error.is_none(),
                        |this| {
                            this.child(self.render_scrolling_text(
                                artist.clone().into(),
                                info_width,
                                px(10.0),
                                FontWeight::NORMAL,
                                artist_id,
                                window,
                            ))
                        },
                    ),
            );

        let controls = h_flex()
            .flex_none()
            .items_center()
            .gap_1()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .occlude()
            .child(
                Button::new("mini-previous")
                    .icon(PlayerIcon::Previous)
                    .tooltip(t!("Previous track"))
                    .ghost()
                    .with_size(px(18.0))
                    .rounded_full()
                    .disabled(!has_track)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.skip(-1);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("mini-play-pause")
                    .icon(if is_playing {
                        PlayerIcon::Pause
                    } else {
                        PlayerIcon::Play
                    })
                    .tooltip(if is_playing { t!("Pause") } else { t!("Play") })
                    .custom(play_button_style)
                    .with_size(px(26.0))
                    .rounded_full()
                    .disabled(!has_track)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_playback();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("mini-next")
                    .icon(PlayerIcon::Next)
                    .tooltip(t!("Next track"))
                    .ghost()
                    .with_size(px(18.0))
                    .rounded_full()
                    .disabled(!has_track)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.skip(1);
                        cx.notify();
                    })),
            );

        // 滚轮调音量后 1 秒内，进度线位置改为显示当前音量，给无面板的迷你窗口
        // 一个即时反馈；平时仍显示播放进度。
        let volume_flash = self
            .mini_volume_flash
            .is_some_and(|at| at.elapsed() < Duration::from_secs(1));
        let (bar_fraction, bar_color) = if volume_flash {
            (
                effective_volume(self.config.volume, self.muted),
                accent.opacity(0.95),
            )
        } else {
            (progress_percent / 100.0, accent.opacity(0.9))
        };
        let progress_line = div()
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .h(px(2.5))
            .bg(accent.opacity(0.2))
            .child(
                div()
                    .h_full()
                    .rounded_r_full()
                    .w(relative(bar_fraction.clamp(0.0, 1.0)))
                    .bg(bar_color),
            );

        let expand_button = Button::new("mini-expand")
            .icon(AppIcon::Maximize)
            .tooltip(t!("Exit mini player"))
            .ghost()
            .with_size(px(18.0))
            .rounded_full()
            .on_click(cx.listener(|this, _, window, cx| {
                this.exit_mini_mode(window, cx);
            }));

        // The mini player root is a window drag area, so the expand button needs to
        // block the drag behind it (like the `controls` row) or its clicks would be
        // turned into window moves.
        let expand_button = h_flex()
            .flex_none()
            .items_center()
            .occlude()
            .child(expand_button);

        v_flex()
            .relative()
            .size_full()
            .border_1()
            .border_color(theme.border.opacity(0.5))
            .text_color(theme.foreground)
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                let delta_y = f32::from(event.delta.pixel_delta(px(16.0)).y);
                if delta_y.abs() < f32::EPSILON {
                    return;
                }
                let delta = if delta_y > 0.0 { 0.05 } else { -0.05 };
                this.adjust_volume(delta, window, cx);
                this.show_mini_volume_flash(cx);
                cx.stop_propagation();
            }))
            .when(cfg!(target_os = "windows"), |this| {
                this.window_control_area(WindowControlArea::Drag)
            })
            .when(cfg!(not(target_os = "windows")), |this| {
                this.on_mouse_down(MouseButton::Left, |_, window, _| window.start_window_move())
            })
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .px_1p5()
                    .gap_1p5()
                    .items_center()
                    .child(cover_element)
                    .child(info)
                    .child(controls)
                    .child(expand_button),
            )
            .child(progress_line)
            .with_animation(
                "mini-player-background",
                bg_animation,
                move |this, delta| {
                    this.bg(linear_gradient(
                        120.0 + delta * 360.0,
                        linear_color_stop(bg_start, 0.0),
                        linear_color_stop(bg_end, 1.0),
                    ))
                },
            )
            .into_any_element()
    }

    pub(super) fn render_player(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let playback = self.audio.state();
        let accent = self.now_playing_accent(cx);
        let accent_foreground = accent_foreground(accent);
        let play_button_style = ButtonCustomVariant::new(cx)
            .color(accent)
            .foreground(accent_foreground)
            .hover(adjust_lightness(accent, 0.05))
            .active(adjust_lightness(accent, -0.05))
            .shadow(true);
        let duration = playback.duration.unwrap_or_default();
        let buffered_percent = if duration.is_zero() {
            0.0
        } else {
            (playback.buffered.as_secs_f32() / duration.as_secs_f32() * 100.0).clamp(0.0, 100.0)
        };
        let queue_open = self.state.queue_drawer_open;
        let viewport_width = f32::from(window.viewport_size().width);
        let compact_player = compact_window_layout(viewport_width);
        let song_info_width = if compact_player { 200.0 } else { 260.0 };
        let player_cover_size = if compact_player { 48.0 } else { 60.0 };
        let right_controls_width = if compact_player { 180.0 } else { 260.0 };
        // 窄窗口优先保证控制按钮的呼吸空间，隐藏格式胶囊（信息仍在
        // 正在播放页可见）。
        let show_technical_chip = viewport_width >= 1080.0;
        let output_volume = effective_volume(self.config.volume, self.muted);
        let volume_icon = if self.muted || output_volume <= 0.001 {
            AppIcon::VolumeMuted
        } else if output_volume < 0.5 {
            AppIcon::VolumeLow
        } else {
            AppIcon::VolumeHigh
        };
        let volume_value = match self.volume_slider.read(cx).value() {
            SliderValue::Single(value) => value,
            SliderValue::Range(_, value) => value,
        };
        let technical_info = self
            .state
            .now_playing
            .as_ref()
            .zip(self.state.now_playing_quality)
            .map(|(song, quality)| playback_technical_info(song, quality));
        let song_info = if let Some(song) = &self.state.now_playing {
            // 悬停提示显示完整标题/艺人（长标题被截断时也能看全文），
            // 并说明点击可打开正在播放页。
            let tooltip_text = if song.artist.trim().is_empty() {
                t!("%{title}\nOpen now playing", title = song.title).to_string()
            } else {
                t!(
                    "%{title} — %{artist}\nOpen now playing",
                    title = song.title,
                    artist = song.artist
                )
                .to_string()
            };
            h_flex()
                .id("now-playing-info")
                .w(px(song_info_width))
                .flex_shrink_0()
                .min_w_0()
                .overflow_hidden()
                .gap(if compact_player { px(8.0) } else { px(12.0) })
                .p_1()
                .items_center()
                .cursor_pointer()
                .rounded(px(12.0))
                .hover(|style| style.bg(cx.theme().accent.opacity(0.08)))
                .tooltip(move |window, cx| Tooltip::new(tooltip_text.clone()).build(window, cx))
                .child(self.render_cover(song.cover_art.as_deref(), player_cover_size, cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .when(compact_player, |this| this.text_sm())
                                .when(!compact_player, |this| this.text_base())
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(song.title.clone()),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(self.song_artist_link(
                                    SharedString::from(format!("player-bar-artist-{}", song.id)),
                                    song,
                                    cx,
                                )),
                        ),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.open_now_playing();
                    cx.notify();
                }))
                .into_any_element()
        } else {
            h_flex()
                .w(px(song_info_width))
                .flex_shrink_0()
                .min_w_0()
                .overflow_hidden()
                .gap(if compact_player { px(8.0) } else { px(12.0) })
                .p_1()
                .items_center()
                .child(self.render_cover(None, player_cover_size, cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .when(compact_player, |this| this.text_sm())
                                .when(!compact_player, |this| this.text_base())
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(t!("No track playing")),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(t!("Choose a song from your library.")),
                        ),
                )
                .into_any_element()
        };

        let right_controls = h_flex()
            .w(px(right_controls_width))
            .h(px(40.0))
            .min_w_0()
            .flex_shrink_0()
            .justify_end()
            .items_center()
            .gap(if compact_player { px(4.0) } else { px(8.0) })
            .when(compact_player, |this| this.pl_2())
            .when(!compact_player, |this| this.pl_3())
            .border_l_1()
            .border_color(cx.theme().border.opacity(0.8))
            .when_some(
                technical_info.filter(|_| show_technical_chip),
                |this, info| {
                    let tooltip = info.tooltip.clone();
                    this.child(
                        h_flex()
                            .id("player-technical-info")
                            .min_w_0()
                            .gap_0p5()
                            .tooltip(move |window, cx| {
                                Tooltip::new(tooltip.clone()).build(window, cx)
                            })
                            .child(technical_info_chip(
                                "player-format",
                                info.format,
                                accent,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .h(px(24.0))
                            .border_l_1()
                            .border_color(cx.theme().border.opacity(0.6)),
                    )
                },
            )
            .when_some(self.state.now_playing.as_ref(), |this, song| {
                this.child(
                    self.favorite_button(FavoriteKind::Song, &song.id, cx)
                        .with_size(px(30.0)),
                )
            })
            .child(self.render_eq_popover(cx))
            .child(
                Button::new("player-queue")
                    .icon(AppIcon::Queue)
                    .tooltip(t!(
                        "Queue (%{shortcut})",
                        shortcut = shortcut_display(
                            self.config.shortcuts.binding(ShortcutAction::ToggleQueue)
                        )
                    ))
                    .custom(self.player_toggle_style(queue_open, cx))
                    .with_size(px(30.0))
                    .rounded_full()
                    .toggled(queue_open)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.settings_open = false;
                        this.state.queue_drawer_open = !this.state.queue_drawer_open;
                        if !this.state.queue_drawer_open {
                            this.clear_song_multi();
                        }
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .h(px(24.0))
                    .border_l_1()
                    .border_color(cx.theme().border.opacity(0.6)),
            )
            .child(
                div()
                    .id("player-volume-control")
                    .relative()
                    .flex_none()
                    .on_hover(cx.listener(|this, hovering, _, cx| {
                        if *hovering {
                            this.show_volume_panel(cx);
                        } else {
                            this.schedule_volume_panel_close(cx);
                        }
                    }))
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                        let delta_y = f32::from(event.delta.pixel_delta(px(16.0)).y);
                        if delta_y.abs() < f32::EPSILON {
                            return;
                        }
                        let delta = if delta_y > 0.0 { 0.05 } else { -0.05 };
                        this.adjust_volume(delta, window, cx);
                        cx.stop_propagation();
                    }))
                    .when(self.volume_panel_open, |this| {
                        this.child(
                            v_flex()
                                .id("player-volume-panel")
                                .absolute()
                                .bottom(px(64.0))
                                .left(px(-13.0))
                                .w(px(54.0))
                                .p_2()
                                .items_center()
                                .gap_1()
                                .rounded(px(6.0))
                                .border_1()
                                .border_color(cx.theme().border)
                                .bg(cx.theme().popover)
                                .text_color(cx.theme().popover_foreground)
                                .shadow_md()
                                .capture_any_mouse_down(cx.listener(|this, _, _, cx| {
                                    this.begin_volume_panel_drag(cx);
                                }))
                                .capture_any_mouse_up(cx.listener(|this, _, _, cx| {
                                    this.end_volume_panel_drag(true, cx);
                                }))
                                .on_mouse_up_out(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, cx| {
                                        this.end_volume_panel_drag(false, cx);
                                    }),
                                )
                                .on_hover(cx.listener(|this, hovering, _, cx| {
                                    if *hovering {
                                        this.show_volume_panel(cx);
                                    } else {
                                        this.schedule_volume_panel_close(cx);
                                    }
                                }))
                                .child(
                                    div()
                                        .h(px(18.0))
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(if self.muted {
                                            cx.theme().muted_foreground
                                        } else {
                                            cx.theme().popover_foreground
                                        })
                                        .child(format!("{}%", volume_value.round() as u32)),
                                )
                                .child(
                                    div()
                                        .h(px(120.0))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(
                                            Slider::new(&self.volume_slider)
                                                .vertical()
                                                .h(px(120.0))
                                                .bg(accent)
                                                .text_color(accent)
                                                .rounded_full(),
                                        ),
                                ),
                        )
                    })
                    .child(
                        Button::new("player-volume-mute")
                            .icon(volume_icon)
                            .tooltip(if self.muted {
                                t!(
                                    "Unmute (%{shortcut})",
                                    shortcut = shortcut_display(
                                        self.config.shortcuts.binding(ShortcutAction::ToggleMute)
                                    )
                                )
                            } else {
                                t!(
                                    "Mute (%{shortcut})",
                                    shortcut = shortcut_display(
                                        self.config.shortcuts.binding(ShortcutAction::ToggleMute)
                                    )
                                )
                            })
                            .ghost()
                            .with_size(px(30.0))
                            .rounded_full()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_mute(window, cx);
                            })),
                    ),
            );

        h_flex()
            .h(px(if compact_player { 82.0 } else { 88.0 }))
            .when(compact_player, |this| this.px_3())
            .when(!compact_player, |this| this.px_4())
            .py_2()
            .gap(if compact_player { px(8.0) } else { px(16.0) })
            .items_center()
            .justify_between()
            .border_t_1()
            .border_color(cx.theme().border.opacity(0.8))
            .bg(cx.theme().secondary.opacity(0.22))
            .child(song_info)
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .gap_1()
                    .items_center()
                    .child(
                        h_flex()
                            .gap_3()
                            .items_center()
                            .child(self.shuffle_button(cx))
                            .child(
                                Button::new("previous")
                                    .icon(PlayerIcon::Previous)
                                    .tooltip(t!(
                                        "Previous track (%{shortcut})",
                                        shortcut = shortcut_display(
                                            self.config.shortcuts.binding(ShortcutAction::Previous)
                                        )
                                    ))
                                    .ghost()
                                    .with_size(px(30.0))
                                    .rounded_full()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.skip(-1);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("play-pause")
                                    .icon(if playback.active && !playback.paused {
                                        PlayerIcon::Pause
                                    } else {
                                        PlayerIcon::Play
                                    })
                                    .tooltip(if playback.active && !playback.paused {
                                        t!(
                                            "Pause (%{shortcut})",
                                            shortcut = shortcut_display(
                                                self.config
                                                    .shortcuts
                                                    .binding(ShortcutAction::TogglePlayback)
                                            )
                                        )
                                    } else {
                                        t!(
                                            "Play (%{shortcut})",
                                            shortcut = shortcut_display(
                                                self.config
                                                    .shortcuts
                                                    .binding(ShortcutAction::TogglePlayback)
                                            )
                                        )
                                    })
                                    .custom(play_button_style)
                                    .with_size(px(40.0))
                                    .rounded_full()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_playback();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("next")
                                    .icon(PlayerIcon::Next)
                                    .tooltip(t!(
                                        "Next track (%{shortcut})",
                                        shortcut = shortcut_display(
                                            self.config.shortcuts.binding(ShortcutAction::Next)
                                        )
                                    ))
                                    .ghost()
                                    .with_size(px(30.0))
                                    .rounded_full()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.skip(1);
                                        cx.notify();
                                    })),
                            )
                            .child(self.repeat_button(cx)),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .h(px(24.0))
                            .max_w(px(760.0))
                            .items_center()
                            .gap_2()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .when_some(playback.stall, |this, stall| {
                                this.child(render_stall_chip(stall, accent))
                            })
                            .when_some(playback.error.clone(), |this, error| {
                                this.child(render_playback_error_chip(error, cx))
                            })
                            .child(
                                div()
                                    .flex_none()
                                    .min_w(px(40.0))
                                    .text_right()
                                    .child(format_playback(playback.position)),
                            )
                            .child(
                                div()
                                    .relative()
                                    .flex_1()
                                    .mx_1()
                                    .h_6()
                                    .child(
                                        div()
                                            .absolute()
                                            .left_0()
                                            .top(px(9.0))
                                            .h_1p5()
                                            .w(relative(buffered_percent / 100.0))
                                            .rounded_full()
                                            .bg(accent.opacity(0.24)),
                                    )
                                    .child(
                                        Slider::new(&self.playback_slider)
                                            .w_full()
                                            .bg(accent)
                                            .text_color(accent)
                                            .rounded_full(),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .min_w(px(40.0))
                                    .child(format_playback(duration)),
                            ),
                    ),
            )
            .child(right_controls)
            .into_any_element()
    }
}

/// 播放中数据停止到达时的紧凑提示：转圈图标 + 已等待秒数。
/// 目的不是报错，而是让用户立刻分清「在等网络」与「卡死」。
fn render_stall_chip(stall: Duration, accent: Hsla) -> gpui_kit::AnyElement {
    let seconds = stall.as_secs();
    let label = if seconds >= 2 {
        t!("Buffering %{seconds}s", seconds = seconds).to_string()
    } else {
        t!("Buffering").to_string()
    };
    h_flex()
        .id("player-stall")
        .flex_none()
        .h(px(20.0))
        .px_1p5()
        .gap_1()
        .items_center()
        .rounded(px(4.0))
        .bg(accent.opacity(0.10))
        .text_size(px(12.0))
        .text_color(accent)
        .child(Icon::new(KitIcon::LoaderCircle).with_size(px(11.0)))
        .child(label)
        .into_any_element()
}

/// 播放失败的紧凑提示 + 立即重试。
/// 播放条只有 24px 高，这里只显示归因摘要，完整原因放在悬停提示里。
fn render_playback_error_chip(error: String, cx: &Context<NavidromeApp>) -> gpui_kit::AnyElement {
    let tooltip = error.clone();
    let summary = error
        .split_once(": ")
        .map_or(error.as_str(), |(summary, _)| summary)
        .to_string();
    h_flex()
        .id("player-error")
        .flex_none()
        .h(px(20.0))
        .gap_1()
        .items_center()
        .text_size(px(12.0))
        .text_color(cx.theme().danger)
        .child(Icon::new(KitIcon::TriangleAlert).with_size(px(11.0)))
        .child(
            div()
                .id("player-error-text")
                .max_w(px(150.0))
                .truncate()
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .child(summary),
        )
        .child(
            Button::new("player-retry")
                .label(t!("Retry"))
                .ghost()
                .xsmall()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.retry_current_track();
                    cx.notify();
                })),
        )
        .into_any_element()
}
