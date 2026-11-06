//! GPUI 主窗口渲染循环与顶层布局。

use super::view_helpers::compact_window_layout;
use super::*;

impl Render for NavidromeApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let playback = self.playback_for_display();
        let cover = self
            .state
            .now_playing
            .as_ref()
            .and_then(|song| song.cover_art.as_ref())
            .and_then(|id| self.state.covers.image(id))
            .cloned()
            .unwrap_or_else(|| self.default_cover.clone());
        self.mark_cover_image_used(&cover);
        let animate_cover = self
            .playback_presentation()
            .animate_cover(&playback, window.is_visible());
        self.vinyl_cover
            .prepare(cover, animate_cover, &self.runtime, window);
        let progress = playback
            .duration
            .filter(|duration| !duration.is_zero())
            .map(|duration| {
                (playback.position.as_secs_f32() / duration.as_secs_f32() * 100.0).clamp(0.0, 100.0)
            })
            .unwrap_or_default();
        self.playback_slider.update(cx, |slider, cx| {
            let SliderValue::Single(current) = slider.value() else {
                return;
            };
            if (current - progress).abs() > 0.5 {
                slider.set_value(progress, window, cx);
            }
        });

        if self.mini_window.is_mini() {
            self.sync_mini_window_size(window);
            return self.render_mini(window, cx);
        }

        let settings_open = self.state.settings.open;
        let viewport_width = f32::from(window.viewport_size().width);
        // 900px 级窗口采用结构化紧凑模式：侧栏只保留图标，释放宽度给
        // 内容和播放器；用户保存的展开偏好在窗口恢复宽度后自动回来。
        let auto_collapsed_sidebar = compact_window_layout(viewport_width);
        let sidebar_collapsed = self.config.sidebar_collapsed || auto_collapsed_sidebar;
        let queue_drawer_width = if auto_collapsed_sidebar { 320.0 } else { 380.0 };

        if self.state.view == View::NowPlaying {
            let (cover_base, cover_accent) = self.now_playing_palette(cx);
            let light_background = cx.theme().background.l >= 0.5;
            let base_strength = if light_background { 0.3 } else { 0.22 };
            let accent_strength = if light_background { 0.24 } else { 0.18 };
            let background_start = cx
                .theme()
                .background
                .blend(cover_base.opacity(base_strength));
            let background_end = cx
                .theme()
                .background
                .blend(cover_accent.opacity(accent_strength));
            let titlebar_strength = if light_background { 0.26 } else { 0.2 };
            let titlebar_start = cx
                .theme()
                .title_bar
                .blend(cover_base.opacity(titlebar_strength));
            let titlebar_end = cx
                .theme()
                .title_bar
                .blend(cover_accent.opacity(titlebar_strength * 0.82));
            let titlebar_border = cx
                .theme()
                .title_bar_border
                .blend(cover_accent.opacity(0.34));
            // Reuse the playback tick instead of requesting display-rate frames
            // for a page-wide gradient (including while paused).
            let background_angle = 120.0 + (playback.position.as_secs_f32() / 22.0).fract() * 360.0;

            return v_flex()
                .size_full()
                .text_color(cx.theme().foreground)
                .child(
                    TitleBar::new()
                        .on_close_window(Self::close_button_handler(cx))
                        .bg(linear_gradient(
                            90.0,
                            linear_color_stop(titlebar_start, 0.0),
                            linear_color_stop(titlebar_end, 1.0),
                        ))
                        .border_color(titlebar_border)
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
                            this.child(self.render_queue_drawer(queue_drawer_width, true, cx))
                        })
                        .when(settings_open, |this| {
                            this.child(self.render_settings_overlay(auto_collapsed_sidebar, cx))
                        }),
                )
                .child(self.render_player(window, cx))
                .bg(linear_gradient(
                    background_angle,
                    linear_color_stop(background_start, 0.0),
                    linear_color_stop(background_end, 1.0),
                ))
                .into_any_element();
        }

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
                    .child(
                        v_flex()
                            .w(px(if sidebar_collapsed { 56.0 } else { 216.0 }))
                            .h_full()
                            .p_3()
                            .gap_1()
                            .border_r_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().sidebar)
                            // 当前服务器：多服务器下必须一眼可见正在浏览谁的库，
                            // 点击进入设置切换。
                            .child(self.render_sidebar_server(sidebar_collapsed, cx))
                            .when(!sidebar_collapsed, |this| {
                                this.child(
                                    div()
                                        .px_2()
                                        .py_2()
                                        .text_xs()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(cx.theme().muted_foreground)
                                        .child(t!("LIBRARY")),
                                )
                            })
                            .child(self.nav_button(View::Home, None, sidebar_collapsed, cx))
                            .child(self.nav_button(
                                View::Favorites,
                                Some(self.state.favorites.total()),
                                sidebar_collapsed,
                                cx,
                            ))
                            .child(self.nav_button(
                                View::Artists,
                                Some(self.state.artists.len()),
                                sidebar_collapsed,
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
                                    sidebar_collapsed,
                                    cx,
                                ),
                            )
                            .child(self.nav_button(
                                View::Genres,
                                Some(self.state.genres.len()),
                                sidebar_collapsed,
                                cx,
                            ))
                            .child(self.nav_button(
                                View::Playlists,
                                Some(self.state.playlists.items().len()),
                                sidebar_collapsed,
                                cx,
                            ))
                            .child(self.nav_button(View::Search, None, sidebar_collapsed, cx))
                            .child(self.sidebar_collapse_toggle(
                                sidebar_collapsed,
                                auto_collapsed_sidebar,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .id("content")
                            .flex_1()
                            .h_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.content_scroll_handle)
                            .on_scroll_wheel(cx.listener(|this, _, _, _| {
                                this.maybe_load_more_content();
                            }))
                            .when(compact_window_layout(viewport_width), |this| this.p_4())
                            .when(!compact_window_layout(viewport_width), |this| this.p_6())
                            .child(self.render_content(window, cx)),
                    )
                    .when(self.state.queue_drawer_open, |row| {
                        // 普通浏览页使用真正的分栏，让专辑网格和歌曲列表
                        // 获得抽屉打开后的实际宽度并自然重排，而不是被覆盖裁切。
                        row.child(self.render_queue_drawer(queue_drawer_width, false, cx))
                    })
                    // 设置为内容区覆盖层：点击背景关闭，标题栏与播放条保持可操作。
                    .when(settings_open, |row| {
                        row.child(self.render_settings_overlay(auto_collapsed_sidebar, cx))
                    }),
            )
            .child(self.render_player(window, cx))
            .into_any_element()
    }
}
