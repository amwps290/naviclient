//! 底部播放控制栏的渲染。

use super::*;

impl NavidromeApp {
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
