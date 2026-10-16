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
            .label(format!("Sort: {}", self.state.albums_sort.label()))
            .icon(AppIcon::ChevronDown)
            .tooltip("Album sort order")
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
            .label(format!("Sort: {}", self.state.artists_sort.label()))
            .icon(AppIcon::ChevronDown)
            .tooltip("Artist sort order")
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
            .hover(|style| style.text_color(cx.theme().accent))
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
                    (Some(albums), Some(songs)) => format!("{albums} albums · {songs} songs"),
                    (Some(albums), None) => format!("{albums} albums"),
                    (None, Some(songs)) => format!("{songs} songs"),
                    (None, None) => "—".to_string(),
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
                    .tooltip("Equalizer")
                    .ghost()
                    .with_size(px(30.0))
                    .rounded_full(),
            )
            .content(move |_, _, popover_cx| {
                let accent = popover_cx.theme().accent;
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
                                    .child("Equalizer"),
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .children([(true, "On"), (false, "Off")].map(
                                        |(enable, label)| {
                                            let app = app.clone();
                                            Button::new(SharedString::from(format!(
                                                "eq-popover-{label}"
                                            )))
                                            .label(label)
                                            .small()
                                            .ghost()
                                            .selected(enabled == enable)
                                            .on_click(move |_, _, cx| {
                                                let _ = app.update(cx, |this, cx| {
                                                    this.set_eq_enabled(enable, cx);
                                                });
                                            })
                                        },
                                    )),
                            ),
                    )
                    // 预设
                    .child(h_flex().gap_1().flex_wrap().children(presets.iter().map(
                        |(name, preset_gains)| {
                            let preset_gains = *preset_gains;
                            let app = app.clone();
                            let sliders = sliders.clone();
                            Button::new(SharedString::from(format!("eq-popover-preset-{name}")))
                                .label(*name)
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
                                            .text_color(if enabled { accent } else { muted })
                                            .child(format!("{:+.0}", gains[band])),
                                    )
                                    .child(Slider::new(slider).vertical().h(px(96.0)).bg(accent))
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
        tooltip: &'static str,
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
                    .label("Loading failed — try again")
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
                                    this.child(format!("- {year}"))
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
                                    .map(|count| format!("{count} albums"))
                                    .unwrap_or_else(|| "Artist".to_string()),
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
            return div().into_any_element();
        }
        let handle = self.song_list_scroll(scope);
        // 专辑 / 播放列表详情和队列页中表格占满剩余高度（表头固定，列表内部滚动）；
        // 收藏与搜索页的歌曲表格与其他分区共存，限制高度独立滚动。
        let fill = matches!(scope, "album-detail" | "playlist-detail" | "queue");
        let rows = cx.processor(move |this, range: Range<usize>, _window, cx| {
            let playback = this.audio.state();
            range
                .map(|index| this.render_song_row(scope, index, editable, &playback, cx))
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
            .border_1()
            .border_color(cx.theme().border)
            .rounded_lg()
            .overflow_hidden()
            .when(fill, |this| this.flex_1().min_h_0())
            .when(self.multi_selection_count(scope) > 0, |this| {
                this.child(self.render_multi_action_bar(scope, cx))
            })
            .child(
                h_flex()
                    .h(px(36.0))
                    .flex_none()
                    .px_3()
                    .gap_3()
                    .bg(cx.theme().secondary)
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().w(px(34.0)))
                    .child(div().w(px(42.0)))
                    .child(div().flex_1().flex_grow(3.0).child("Title"))
                    .child(
                        div()
                            .flex_1()
                            .flex_grow(2.0)
                            .max_w(px(180.0))
                            .child("Artist"),
                    )
                    .child(div().w(px(64.0)).text_right().child("Time"))
                    .child(div().w(px(if editable { 124.0 } else { 92.0 }))),
            )
            .child(list)
            .child(Scrollbar::vertical(&handle))
            .into_any_element()
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
            "Unfavorite selected"
        } else {
            "Favorite selected"
        };
        let add_disabled = self.playlist_ui.busy || self.api.is_none();
        h_flex()
            .h(px(44.0))
            .flex_none()
            .w_full()
            .px_3()
            .gap_2()
            .items_center()
            .bg(cx.theme().accent.opacity(0.16))
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("{} selected", songs.len())),
            )
            .child(div().flex_1())
            .child(
                Button::new(SharedString::from(format!("multi-play-{scope}")))
                    .icon(PlayerIcon::Play)
                    .tooltip("Play selected (Enter)")
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
                    .tooltip("Play selected next")
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
                    .tooltip("Add selected to queue")
                    .ghost()
                    .small()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected_to_queue(scope);
                        cx.notify();
                    })),
            )
            .child(
                Button::new(SharedString::from(format!("multi-playlist-{scope}")))
                    .label("Add to playlist…")
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
                    .tooltip("Clear selection (Esc)")
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
        playback: &PlaybackState,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
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
        let album = if song.album.trim().is_empty() {
            "Unknown album".to_string()
        } else {
            song.album.clone()
        };
        // uniform_list 把每行当作独立根元素布局（Definite 可用宽度），
        // 不会像普通容器那样横向拉伸，必须显式 w_full：否则行宽收缩为
        // 内容宽度，行尾按钮、悬停/正在播放高亮都无法纵向对齐。
        let row = h_flex()
            .id(SharedString::from(key.clone()))
            .w_full()
            .h(px(60.0))
            .px_3()
            .gap_3()
            .relative()
            .border_t_1()
            .border_color(cx.theme().border.opacity(0.6))
            .hover(|style| style.bg(cx.theme().accent.opacity(0.12)))
            .cursor_pointer();
        let keyboard_selected =
            self.state
                .song_list_selected
                .is_some_and(|(selected_scope, selected_index)| {
                    selected_scope == scope && selected_index == index
                });
        let multi_selected =
            self.state
                .song_list_multi
                .as_ref()
                .is_some_and(|(selected_scope, indices)| {
                    *selected_scope == scope && indices.contains(&index)
                });
        // 高亮优先级：多选 > 键盘光标 > 正在播放。
        let row = if multi_selected {
            row.bg(cx.theme().accent.opacity(0.28))
        } else if keyboard_selected {
            row.bg(cx.theme().accent.opacity(0.16))
        } else if current {
            row.bg(cx.theme().info.opacity(0.1))
        } else {
            row
        };
        row.child(
            div()
                .w(px(34.0))
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .child(self.favorite_button(FavoriteKind::Song, &song.id, cx)),
        )
        .child(
            div()
                .w(px(42.0))
                .h(px(42.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded_lg()
                .border_1()
                .border_color(cx.theme().border.opacity(0.7))
                .bg(cx.theme().background)
                .child(self.render_cover(song.cover_art.as_deref(), 36.0, cx)),
        )
        .child(
            h_flex()
                .flex_1()
                .flex_grow(3.0)
                .min_w_0()
                .gap_2()
                .text_color(if current {
                    cx.theme().info
                } else {
                    cx.theme().foreground
                })
                .child(self.render_playing_indicator(&key, current, animated, cx))
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
                                .child(self.song_album_link(
                                    SharedString::from(format!("song-row-album-{key}")),
                                    album,
                                    song,
                                    cx,
                                )),
                        ),
                ),
        )
        .child(
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
        .child(
            div()
                .w(px(64.0))
                .text_sm()
                .text_right()
                .text_color(cx.theme().muted_foreground)
                .child(format_duration(song.duration)),
        )
        .child(
            // 行尾操作列：固定宽度、常规布局，不再用悬浮层盖住行内容。
            h_flex()
                .flex_none()
                .h_full()
                .w(px(if editable { 124.0 } else { 92.0 }))
                .items_center()
                .justify_end()
                .gap_0p5()
                .when(editable, |slot| {
                    slot.child(
                        Button::new(SharedString::from(format!("queue-up-{key}")))
                            .icon(AppIcon::ChevronUp)
                            .tooltip("Move up")
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
                            .tooltip("Move down")
                            .ghost()
                            .small()
                            .disabled(index + 1 >= total)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.move_queue_item(index, index + 1);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("queue-remove-{key}")))
                            .icon(AppIcon::Close)
                            .tooltip("Remove from queue")
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
                .when(!editable, |slot| {
                    slot.child(
                        Button::new(SharedString::from(format!("play-next-{key}")))
                            .icon(PlayerIcon::Next)
                            .tooltip("Play next")
                            .ghost()
                            .small()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(song) = this.song_for_scope(scope, index) {
                                    this.insert_next(song.clone());
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new(SharedString::from(format!("add-to-queue-{key}")))
                            .icon(AppIcon::Queue)
                            .tooltip("Add to queue")
                            .ghost()
                            .small()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(song) = this.song_for_scope(scope, index) {
                                    this.add_to_queue(song.clone());
                                }
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
                                .tooltip("Song actions")
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
                let modifiers = event.modifiers();
                if modifiers.control && !modifiers.alt && !modifiers.platform {
                    this.toggle_song_multi(scope, index);
                } else if modifiers.shift && !modifiers.alt && !modifiers.platform {
                    this.range_song_multi(scope, index);
                } else {
                    // 普通单击：退出多选并立即播放该歌曲
                    this.clear_song_multi();
                    this.play_song_at_scope(scope, index);
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
    pub(super) fn render_server_status_card(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let status_label = if self.state.loading {
            "Connecting..."
        } else if self.state.server.is_some() {
            "Connected"
        } else {
            "Not connected"
        };
        let status_color = if self.state.server.is_some() {
            cx.theme().accent
        } else {
            cx.theme().muted_foreground
        };
        let server_name = self
            .state
            .server
            .as_ref()
            .map(|server| {
                format!(
                    "{} {}",
                    server.app_name.as_deref().unwrap_or("Navidrome"),
                    server.version
                )
            })
            .unwrap_or_else(|| "Server entry".to_string());
        let server_url = if self.config.server_url.trim().is_empty() {
            "No server URL configured".to_string()
        } else {
            self.config.server_url.clone()
        };
        let username = if self.config.username.trim().is_empty() {
            "No username configured".to_string()
        } else {
            self.config.username.clone()
        };

        v_flex()
            .w_full()
            .gap_4()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary.opacity(0.35))
            .child(
                h_flex()
                    .justify_between()
                    .items_start()
                    .gap_4()
                    .child(
                        v_flex()
                            .min_w_0()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Primary server"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(server_name),
                            )
                            .child(div().text_xs().text_color(status_color).child(status_label)),
                    )
                    .when(self.api.is_some(), |this| {
                        this.child(
                            Button::new("settings-refresh")
                                .icon(AppIcon::Refresh)
                                .tooltip("Refresh library")
                                .loading(self.state.loading)
                                .ghost()
                                .small()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.refresh_library();
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_6()
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Server URL"),
                            )
                            .child(div().text_sm().truncate().child(server_url)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Username"),
                            )
                            .child(div().text_sm().truncate().child(username)),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.state.status.clone()),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Server URL"),
                    )
                    .child(Input::new(&self.server_input).w_full()),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Username"),
                    )
                    .child(Input::new(&self.username_input).w_full()),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Password"),
                    )
                    .child(Input::new(&self.password_input).w_full()),
            )
            .child(
                h_flex().justify_end().gap_2().child(
                    Button::new("save-settings")
                        .label("Save and connect")
                        .primary()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.save_settings(cx);
                            cx.notify();
                        })),
                ),
            )
            .into_any_element()
    }

    /// 设置覆盖层：盖住侧边栏与内容区，点击背景或 Esc 关闭；
    /// 标题栏和底部播放条不被遮挡，改设置时也能控制播放。
    pub(super) fn render_settings_overlay(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
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
                    cx.notify();
                }),
            )
            .child(
                v_flex()
                    .id("settings-card")
                    .w(px(760.0))
                    .max_w(relative(0.92))
                    .max_h(relative(0.88))
                    .mt(px(48.0))
                    .bg(cx.theme().background)
                    .border_1()
                    .border_color(cx.theme().border)
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
                            .px_5()
                            .py_3()
                            .justify_between()
                            .items_center()
                            .border_b_1()
                            .border_color(cx.theme().border)
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Settings"),
                            )
                            .child(
                                Button::new("close-settings")
                                    .icon(AppIcon::Close)
                                    .tooltip("Close settings")
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.state.settings_open = false;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        // 原生 overflow_y_scroll + 手动滚动条图层：
                        // overflow_y_scrollbar 的包装器根节点是 size_full，
                        // 在自动高度的卡片里百分比解析为 0 会整体塌缩。
                        div()
                            .id("settings-body")
                            .min_h_0()
                            .overflow_y_scroll()
                            .track_scroll(&self.settings_scroll)
                            .vertical_scrollbar(&self.settings_scroll)
                            .p_6()
                            .child(self.render_settings(cx)),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let cache_dir = config::audio_cache_dir(&self.config);
        v_flex()
            .w_full()
            .max_w(px(840.0))
            .gap_6()
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Servers"),
                    )
                    .child(self.render_server_status_card(cx)),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Playback"),
                    )
                    .child(
                        v_flex()
                            .gap_4()
                            .p_4()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().secondary.opacity(0.2))
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Audio cache folder"),
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
                                                    .label("Choose folder")
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.choose_cache_directory(cx);
                                                    })),
                                            )
                                            .child(
                                                Button::new("reset-cache-directory")
                                                    .label("Use default")
                                                    .disabled(self.config.cache_dir.is_none())
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.apply_cache_directory(None);
                                                        cx.notify();
                                                    })),
                                            ),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Streaming quality"),
                                    )
                                    .child(h_flex().gap_2().flex_wrap().children(
                                        TranscodingQuality::ALL.into_iter().map(|quality| {
                                            Button::new(SharedString::from(format!(
                                                "quality-{}",
                                                quality.cache_profile()
                                            )))
                                            .label(quality.label())
                                            .selected(self.config.transcoding_quality == quality)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.set_transcoding_quality(quality, cx);
                                            }))
                                        }),
                                    )),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Audio output"),
                    )
                    .child(
                        v_flex()
                            .gap_4()
                            .p_4()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().secondary.opacity(0.2))
                            .child({
                                let playback = self.audio.state();
                                let active = match playback.output_name.as_deref() {
                                    Some(name) => name.to_string(),
                                    None => "System default".to_string(),
                                };
                                div().text_xs().text_color(cx.theme().muted_foreground).child(
                                    format!(
                                        "Active: {active} · {} Hz · {} ch",
                                        playback.output_sample_rate, playback.output_channels
                                    ),
                                )
                            })
                            .child(
                                h_flex()
                                    .gap_2()
                                    .flex_wrap()
                                    .child(
                                        Button::new("output-device-default")
                                            .label("System default")
                                            .tooltip("Follow the OS default output device")
                                            .selected(self.config.output_device.is_none())
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.set_output_device(None, cx);
                                            })),
                                    )
                                    .children(
                                        self.state
                                            .output_devices
                                            .iter()
                                            .enumerate()
                                            .map(|(index, (name, is_default))| {
                                                let name_for_click = name.clone();
                                                // 按钮元素 id 用索引：设备名可能含特殊字符。
                                                Button::new(SharedString::from(format!(
                                                    "output-device-{index}"
                                                )))
                                                .label(format!(
                                                    "{name}{}",
                                                    if *is_default { " (default)" } else { "" }
                                                ))
                                                .selected(
                                                    self.config.output_device
                                                        .as_deref()
                                                        .is_some_and(|selected| selected == name)
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.set_output_device(
                                                        Some(name_for_click.clone()),
                                                        cx,
                                                    );
                                                }))
                                            }),
                                    )
                                    .child(
                                        Button::new("refresh-output-devices")
                                            .label("Refresh devices")
                                            .ghost()
                                            .small()
                                            .loading(self.state.output_devices_loading)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.refresh_output_devices(cx);
                                            })),
                                    ),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Volume normalization"),
                    )
                    .child(
                        v_flex()
                            .gap_4()
                            .p_4()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().secondary.opacity(0.2))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        "Adjust per-song loudness with ReplayGain so quiet and loud tracks play at a similar level.",
                                    ),
                            )
                            .child(
                                h_flex().gap_2().flex_wrap().children(
                                    VolumeNormalization::ALL.into_iter().map(|mode| {
                                        Button::new(SharedString::from(format!(
                                            "volume-normalization-{}",
                                            mode.label().to_ascii_lowercase()
                                        )))
                                        .label(mode.label())
                                        .tooltip(mode.tooltip())
                                        .selected(self.config.volume_normalization == mode)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.set_volume_normalization(mode, cx);
                                        }))
                                    }),
                                ),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Lyrics"),
                    )
                    .child(
                        v_flex()
                            .gap_4()
                            .p_4()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().secondary.opacity(0.2))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(
                                        "Show translated lyrics under the original line when the server provides another language variant.",
                                    ),
                            )
                            .child(
                                h_flex().gap_2().children(
                                    [("Show translation", true), ("Original only", false)]
                                        .into_iter()
                                        .map(|(label, show)| {
                                            Button::new(SharedString::from(format!(
                                                "lyrics-translation-{show}"
                                            )))
                                            .label(label)
                                            .selected(
                                                self.config.show_lyrics_translation == show,
                                            )
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.set_show_lyrics_translation(show, cx);
                                            }))
                                        }),
                                ),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::MEDIUM)
                            .child("Appearance"),
                    )
                    .child(
                        v_flex()
                            .gap_3()
                            .p_4()
                            .rounded_lg()
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().secondary.opacity(0.2))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Theme"),
                            )
                            .child(
                                h_flex().gap_2().children(
                                    [
                                        ThemePreference::Light,
                                        ThemePreference::Dark,
                                        ThemePreference::System,
                                    ]
                                    .into_iter()
                                    .map(|preference| {
                                        Button::new(SharedString::from(format!(
                                            "theme-{}",
                                            preference.label().to_lowercase()
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
            )
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
                        .child("No track playing"),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Choose a song from your library."),
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
                .child("Loading lyrics...")
                .into_any_element()
        } else if let Some(error) = &self.state.lyrics_error {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(cx.theme().muted_foreground)
                .child("Lyrics are unavailable for this track.")
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
                    let text = if lyric_line.text.is_empty() {
                        " ".to_string()
                    } else {
                        lyric_line.text.clone()
                    };
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
                .child("No lyrics are available for this track.")
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
                                    .child("Lyrics"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground.opacity(0.82))
                                    .child(format!("{} - {}", song.title, song.artist)),
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
        title: &str,
        content: gpui_kit::AnyElement,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let collapsed = self.state.collapsed_sections.contains(id);
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .cursor_pointer()
                    .id(SharedString::from(format!("home-section-{id}")))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_home_section(id, cx);
                    }))
                    .child(
                        Icon::new(if collapsed {
                            AppIcon::ChevronRight
                        } else {
                            AppIcon::ChevronDown
                        })
                        .size(px(16.0))
                        .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title.to_string()),
                    ),
            )
            .when(!collapsed, |this| this.child(content))
            .into_any_element()
    }

    /// Home 页专辑区块的通用内容：加载骨架 / 空状态提示 / 专辑网格。
    pub(super) fn render_home_album_content(
        &self,
        scope: &str,
        albums: &[Album],
        loading: bool,
        empty_text: &str,
        cx: &Context<Self>,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        if albums.is_empty() && loading {
            self.render_album_skeleton_grid(cx)
        } else if albums.is_empty() {
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(empty_text.to_string())
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

    pub(super) fn render_content(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        match self.state.view {
            View::Home => v_flex()
                .gap_5()
                .child(
                    self.page_header(
                        "Home",
                        self.state
                            .server
                            .as_ref()
                            .map(|info| format!("Connected to Navidrome API {}", info.version))
                            .unwrap_or_else(|| self.state.status.clone()),
                        cx,
                    ),
                )
                .child(self.render_home_section(
                    "recent",
                    "Recently played",
                    self.render_home_album_content(
                        "recent",
                        &self.state.recent_albums,
                        self.state.recent_albums_loading,
                        "Nothing played yet — recently played albums will appear here.",
                        cx,
                        window,
                    ),
                    cx,
                ))
                .child(self.render_home_section(
                    "frequent",
                    "Frequently played",
                    self.render_home_album_content(
                        "frequent",
                        &self.state.frequent_albums,
                        self.state.frequent_albums_loading,
                        "Nothing frequently played yet — your most-played albums will appear here.",
                        cx,
                        window,
                    ),
                    cx,
                ))
                .child(self.render_home_section(
                    "random",
                    "Random albums",
                    self.render_home_album_content(
                        "random",
                        &self.state.random_albums,
                        self.state.random_albums_loading,
                        "No random albums to show — refresh the library for a fresh pick.",
                        cx,
                        window,
                    ),
                    cx,
                ))
                .child(self.render_home_section(
                    "newest",
                    "Newest albums",
                    self.render_home_album_content(
                        "newest",
                        &self.state.newest_albums,
                        self.state.newest_albums_loading,
                        "No albums in your library yet.",
                        cx,
                        window,
                    ),
                    cx,
                ))
                .into_any_element(),
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
                            "Favorites",
                            format!("{total} starred items from your Navidrome library"),
                            cx,
                        )))
                        .child(self.header_refresh_button(
                            "refresh-favorites",
                            "Refresh favorites",
                            false,
                            cx,
                            |this, _cx| this.load_favorites(),
                        )),
                );

                if total == 0 {
                    content = content.child(
                        div()
                            .py_8()
                            .text_color(cx.theme().muted_foreground)
                            .child("Click a heart on a song, album, or artist to add it here."),
                    );
                }
                if !favorites.artists.is_empty() {
                    content = content
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Artists"),
                        )
                        .child(self.render_artist_grid(&favorites.artists, cx, window));
                }
                if !favorites.albums.is_empty() {
                    content = content
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Albums"),
                        )
                        .child(self.render_album_grid("favorites", &favorites.albums, cx, window));
                }
                if !favorites.songs.is_empty() {
                    content = content
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Songs"),
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
                            "Artists",
                            format!("{} artists in your library", self.state.artists.len()),
                            cx,
                        )))
                        .child(self.render_artist_sort_button(cx))
                        .child(self.header_refresh_button(
                            "refresh-artists",
                            "Refresh artists",
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
                            "Albums",
                            format!("{} albums in your library", self.state.albums.len()),
                            cx,
                        )))
                        .child(self.render_album_sort_button(cx))
                        .child(self.header_refresh_button(
                            "refresh-albums",
                            "Refresh albums",
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
                    format!("{} genres — refreshing…", self.state.genres.len())
                } else {
                    format!("{} genres in your library", self.state.genres.len())
                };
                v_flex()
                    .gap_5()
                    .child(
                        h_flex()
                            .w_full()
                            .items_end()
                            .gap_3()
                            .child(
                                div()
                                    .flex_1()
                                    .child(self.page_header("Genres", subtitle, cx)),
                            )
                            .child(self.header_refresh_button(
                                "refresh-genres",
                                "Refresh genres",
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
                                .child("No genres in your library yet.")
                                .into_any_element()
                        } else {
                            self.render_genre_grid(cx)
                        },
                    )
                    .into_any_element()
            }
            View::GenreDetail => {
                let Some(genre) = self.state.current_genre.clone() else {
                    return div().child("Genre not found").into_any_element();
                };
                v_flex()
                    .gap_5()
                    .child(self.page_header(
                        genre,
                        format!("{} albums", self.state.genre_albums.len()),
                        cx,
                    ))
                    .child(
                        if self.state.genre_albums.is_empty() && self.state.genre_albums_loading {
                            self.render_album_skeleton_grid(cx)
                        } else if self.state.genre_albums.is_empty() {
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child("No albums in this genre yet.")
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
                                    .map(|owner| format!("{track_count} tracks | {owner}"))
                                    .unwrap_or_else(|| format!("{track_count} tracks"));

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
                        "Playlists",
                        format!("{} playlists", self.state.playlists.len()),
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("new-playlist")
                                    .label("New playlist")
                                    .primary()
                                    .small()
                                    .disabled(self.playlist_ui.busy || self.api.is_none())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.name_playlist(None, vec![], window, cx)
                                    })),
                            )
                            .child(
                                Button::new("refresh-playlists")
                                    .icon(AppIcon::Refresh)
                                    .label("Refresh")
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
                                    .child("No playlists are available."),
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
                        "Search",
                        "Find music across the entire server library.",
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
                                    .label("Search")
                                    .primary()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.submit_search(cx);
                                        cx.notify();
                                    })),
                            )
                            .child(self.header_refresh_button(
                                "refresh-search",
                                "Run search again",
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
                                        .child("Artists"),
                                )
                                .child(self.render_artist_grid(&results.artists, cx, window));
                        } else if filter == SearchFilter::Artists {
                            content = content.child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("No artists matched your search."),
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
                                        .child("Albums"),
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
                                    .child("No albums matched your search."),
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
                                        .child("Songs"),
                                )
                                .child(self.render_song_list("search", false, cx));
                        } else if filter == SearchFilter::Songs {
                            content = content.child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("No songs matched your search."),
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
                                .child("No results matched your search."),
                        );
                    }
                }
                content.into_any_element()
            }
            View::ArtistDetail => {
                let Some(artist) = &self.state.current_artist else {
                    return div().child("Artist not found").into_any_element();
                };
                v_flex()
                    .gap_5()
                    .child(self.page_header(
                        artist.name.clone(),
                        format!("{} albums", self.state.artist_albums.len()),
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
                    return div().child("Album not found").into_any_element();
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
                                                this.child(format!("- {year}"))
                                            }),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .flex_wrap()
                                            .child(
                                                Button::new("play-album")
                                                    .label("Play album")
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
                                                    .label("Append all")
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
                                                    .label("Add to playlist…")
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
                    return div().child("Playlist not found").into_any_element();
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
                        format!("{} tracks", self.state.playlist_songs.len()),
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                Button::new("play-playlist")
                                    .icon(PlayerIcon::Play)
                                    .label("Play all")
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
                                    .label("Append all")
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
                                    .label("Rename")
                                    .small()
                                    .outline()
                                    .disabled(edit_disabled)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.name_playlist(Some(rename.clone()), vec![], window, cx)
                                    })),
                            )
                            .child(
                                Button::new("delete-playlist")
                                    .label("Delete")
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
                                    .tooltip("Refresh playlist")
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
                                "Saving playlist…"
                            } else if self.playlist_ui.detail_loading {
                                "Loading playlist…"
                            } else if read_only {
                                "Shared playlist · Only the owner can edit it."
                            } else if self.state.playlist_songs.is_empty() {
                                "This playlist is empty. Use a song’s actions menu to add tracks."
                            } else {
                                "Use a song’s actions menu to move or remove it."
                            }),
                    )
                    .child(self.render_song_list("playlist-detail", false, cx))
                    .into_any_element()
            }
            View::NowPlaying => self.render_now_playing(window, cx),
            View::Queue => v_flex()
                .h_full()
                .min_h_0()
                .gap_5()
                .child(self.page_header(
                    "Queue",
                    format!("{} tracks in the current queue", self.state.queue.len()),
                    cx,
                ))
                .when(self.state.queue.is_empty(), |this| {
                    this.child(
                        div()
                            .py_8()
                            .text_color(cx.theme().muted_foreground)
                            .child("Start playing an album or playlist to build a queue."),
                    )
                })
                .when(!self.state.queue.is_empty(), |this| {
                    this.child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                Button::new("queue-play-all")
                                    .icon(PlayerIcon::Play)
                                    .label("Play all")
                                    .small()
                                    .info()
                                    .outline()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let songs = this.state.queue.clone();
                                        if !songs.is_empty() {
                                            this.play_song_list(&songs, 0);
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("queue-clear-after")
                                    .label("Clear after current")
                                    .small()
                                    .outline()
                                    .disabled(self.state.queue_index.is_none())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.clear_queue_after_current();
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("save-queue-playlist")
                                    .label("Save as playlist")
                                    .small()
                                    .outline()
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
                            .child(
                                Button::new("queue-clear-all")
                                    .icon(AppIcon::Close)
                                    .label("Clear queue")
                                    .small()
                                    .ghost()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.clear_queue();
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(self.render_song_list("queue", true, cx))
                })
                .into_any_element(),
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
            .unwrap_or_else(|| "No track playing".to_string());
        let artist = song
            .as_ref()
            .map(|song| song.artist.clone())
            .unwrap_or_else(|| "Choose a song".to_string());

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
                div()
                    .text_color(theme.muted_foreground)
                    .child(self.render_scrolling_text(
                        artist.clone().into(),
                        info_width,
                        px(10.0),
                        FontWeight::NORMAL,
                        artist_id,
                        window,
                    )),
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
                    .tooltip("Previous track")
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
                    .tooltip(if is_playing { "Pause" } else { "Play" })
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
                    .tooltip("Next track")
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
            .tooltip("Exit mini player")
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
        let queue_open = self.state.view == View::Queue;
        // 窄窗口优先保证控制按钮的呼吸空间，隐藏格式胶囊（信息仍在
        // 正在播放页可见）。
        let show_technical_chip = f32::from(window.viewport_size().width) >= 1080.0;
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
                format!("{}\nOpen now playing", song.title)
            } else {
                format!("{} — {}\nOpen now playing", song.title, song.artist)
            };
            h_flex()
                .id("now-playing-info")
                .w(px(260.0))
                .flex_shrink_0()
                .min_w_0()
                .overflow_hidden()
                .gap_3()
                .p_1()
                .items_center()
                .cursor_pointer()
                .rounded(px(12.0))
                .hover(|style| style.bg(cx.theme().accent.opacity(0.08)))
                .tooltip(move |window, cx| Tooltip::new(tooltip_text.clone()).build(window, cx))
                .child(self.render_cover(song.cover_art.as_deref(), 60.0, cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_base()
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
                .w(px(260.0))
                .flex_shrink_0()
                .min_w_0()
                .overflow_hidden()
                .gap_3()
                .p_1()
                .items_center()
                .child(self.render_cover(None, 60.0, cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child("No track playing"),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child("Choose a song from your library."),
                        ),
                )
                .into_any_element()
        };

        let right_controls = h_flex()
            .w(px(260.0))
            .h(px(40.0))
            .min_w_0()
            .flex_shrink_0()
            .justify_end()
            .items_center()
            .gap_2()
            .pl_3()
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
                    .tooltip("Queue (Q)")
                    .ghost()
                    .with_size(px(30.0))
                    .rounded_full()
                    .selected(queue_open)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.settings_open = false;
                        this.state.view = View::Queue;
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
                            .tooltip(if self.muted { "Unmute (M)" } else { "Mute (M)" })
                            .ghost()
                            .with_size(px(30.0))
                            .rounded_full()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.toggle_mute(window, cx);
                            })),
                    ),
            );

        h_flex()
            .h(px(88.0))
            .px_4()
            .py_2()
            .gap_4()
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
                                    .tooltip("Previous track (Ctrl+Left)")
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
                                        "Pause (Space)"
                                    } else {
                                        "Play (Space)"
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
                                    .tooltip("Next track (Ctrl+Right)")
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
