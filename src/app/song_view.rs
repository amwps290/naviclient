//! 歌曲列表、行交互、多选操作和歌曲详情渲染。

use super::*;

impl NavidromeApp {
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
