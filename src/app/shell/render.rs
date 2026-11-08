//! GPUI 主窗口渲染循环与顶层布局。

use crate::app::prelude::*;
use crate::app::style::cover_tint::{cover_tint, tint_background, TintSurface};

impl Render for NavidromeApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let playback = self.playback_for_display();
        let cover = self
            .state
            .now_playing
            .as_ref()
            .and_then(|song| self.song_cover_image(song))
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
        let layout = self.shell_layout(window);

        if self.state.view == View::NowPlaying {
            let tint = cover_tint(
                TintSurface::NowPlaying,
                self.now_playing_cover_palette(),
                cx.theme().background,
                cx.theme().is_dark(),
            );

            return v_flex()
                .size_full()
                .text_color(cx.theme().foreground)
                .bg(tint_background(tint, cx.theme().background, 120.0))
                .child(
                    TitleBar::new()
                        .on_close_window(Self::close_button_handler(cx))
                        .bg(cx.theme().transparent)
                        .border_color(cx.theme().transparent)
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
                            this.child(self.render_queue_drawer(
                                layout.queue_drawer_width,
                                true,
                                cx,
                            ))
                        })
                        .when(settings_open, |this| {
                            this.child(self.render_settings_overlay(layout.auto_collapsed, cx))
                        }),
                )
                .child(self.render_player(window, cx))
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
                    .child(self.render_sidebar(&layout, cx))
                    .child(
                        div()
                            .id("content")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.content_scroll_handle)
                            .on_scroll_wheel(cx.listener(|this, _, _, _| {
                                this.maybe_load_more_content();
                            }))
                            .when(!self.state.view.has_detail_hero(), |this| {
                                this.p(px(layout.page_padding))
                            })
                            .child(self.render_content(window, cx)),
                    )
                    .when(self.state.queue_drawer_open, |row| {
                        // 普通浏览页使用真正的分栏，让专辑网格和歌曲列表
                        // 获得抽屉打开后的实际宽度并自然重排，而不是被覆盖裁切。
                        row.child(self.render_queue_drawer(layout.queue_drawer_width, false, cx))
                    })
                    // 设置为内容区覆盖层：点击背景关闭，标题栏与播放条保持可操作。
                    .when(settings_open, |row| {
                        row.child(self.render_settings_overlay(layout.auto_collapsed, cx))
                    }),
            )
            .child(self.render_player(window, cx))
            .into_any_element()
    }
}
