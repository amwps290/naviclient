//! 迷你播放器和底部播放控制栏的渲染。

use super::*;

impl NavidromeApp {
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
