//! 通用媒体库组件、专辑/艺术家网格和歌曲列表渲染。
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
        let queue_drawer = matches!(scope, "queue" | "history") && self.state.queue_drawer_open;
        let compact_columns = queue_drawer || compact_song_columns(list_width);
        // 专辑 / 播放列表详情和队列页中表格占满剩余高度（表头固定，列表内部滚动）；
        // 收藏与搜索页的歌曲表格与其他分区共存，限制高度独立滚动。
        let fill = matches!(
            scope,
            "album-detail" | "playlist-detail" | "queue" | "history"
        );
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
        let queue_drawer = matches!(scope, "queue" | "history") && self.state.queue_drawer_open;
        let recommendation_label = (scope == "queue")
            .then(|| self.state.queue_origins.get(index).copied())
            .flatten()
            .and_then(|origin| match origin {
                QueueOrigin::DailyMix => Some(t!("Daily Mix")),
                QueueOrigin::SongRadio => Some(t!("Song radio")),
                QueueOrigin::ArtistRadio => Some(t!("Artist radio")),
                QueueOrigin::AutoContinue => Some(t!("Auto")),
                QueueOrigin::Manual | QueueOrigin::Playlist => None,
            });
        let Some(song) = self.song_for_scope(scope, index) else {
            return div().into_any_element();
        };
        let total = self.songs_for_scope(scope).map_or(0, <[Song]>::len);
        let key = item_instance_key(scope, "song", index, &song.id);
        let menu_app = cx.entity().downgrade();
        let button_menu_app = menu_app.clone();
        let current_song = self
            .state
            .now_playing
            .as_ref()
            .is_some_and(|playing| playing.id == song.id);
        let current = if scope == "history" {
            index == 0 && current_song
        } else {
            current_song && (!editable || self.state.queue_index == Some(index))
        };
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
                            h_flex()
                                .min_w_0()
                                .gap_1p5()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .line_height(rems(1.25))
                                        .truncate()
                                        .child(song.title.clone()),
                                )
                                .when_some(recommendation_label, |this, label| {
                                    this.child(
                                        div()
                                            .flex_none()
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded_full()
                                            .bg(row_accent.opacity(0.1))
                                            .text_xs()
                                            .text_color(row_accent)
                                            .child(label),
                                    )
                                }),
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
}
