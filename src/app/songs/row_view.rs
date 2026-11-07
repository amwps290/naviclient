//! Song row layout, current/selected highlights and event targets.

use super::list::SongList;
use super::row_actions::SongRowTarget;
use crate::app::prelude::*;
use crate::app::shell::view_helpers::{
    item_instance_key, song_row_click_action, SongRowClickAction, SONG_ROW_HEIGHT,
};

impl NavidromeApp {
    /// Resolve the live scoped row or queue occurrence when handling an event.
    pub(in crate::app) fn render_song_row(
        &self,
        scope: SongScope,
        index: usize,
        compact_columns: bool,
        playback: &PlaybackState,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let editable = scope == SongScope::Queue;
        let Some(target) = SongRowTarget::capture(&self.state, scope, index) else {
            return div().into_any_element();
        };
        let queue_drawer = scope.is_drawer() && self.state.queue_drawer_open;
        let recommendation_label = (scope == SongScope::Queue)
            .then(|| self.state.queue.origin(index))
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
        let total = self.state.songs_for_scope(scope).map_or(0, SongList::len);
        let key = item_instance_key(scope.id(), "song", index, &song.id);
        let menu_app = cx.entity().downgrade();
        let button_menu_app = menu_app.clone();
        let current_song = self
            .state
            .now_playing
            .as_ref()
            .is_some_and(|playing| playing.id == song.id);
        let current = if scope == SongScope::History {
            index == 0 && current_song
        } else {
            current_song && (!editable || self.state.queue_index == Some(index))
        };
        let animated = current && playback.active && !playback.paused;
        let p = palette(cx);
        let album = if song.album.trim().is_empty() {
            t!("Unknown album").to_string()
        } else {
            song.album.clone()
        };
        let keyboard_selected =
            self.state
                .song_selection
                .cursor()
                .is_some_and(|(selected_scope, selected_index)| {
                    selected_scope == scope && selected_index == index
                });
        let multi_selected = self.state.song_selection.contains(scope, index);
        // 高亮优先级：多选 > 列表光标 > 正在播放，三者都用品牌淡底；
        // 已带状态的行悬浮时不再变色，文字对比度始终有保证。
        let highlighted = multi_selected || keyboard_selected || current;
        // uniform_list 把每行当作独立根元素布局（Definite 可用宽度），
        // 不会像普通容器那样横向拉伸，必须显式 w_full：否则行宽收缩为
        // 内容宽度，行尾按钮、悬停/正在播放高亮都无法纵向对齐。
        let row = h_flex()
            .id(SharedString::from(key.clone()))
            .w_full()
            .h(px(SONG_ROW_HEIGHT))
            .px_3()
            .gap(if queue_drawer { px(8.0) } else { px(12.0) })
            .relative()
            .rounded(px(tokens::radius::ROW))
            .when(highlighted, |this| this.bg(p.brand_soft))
            .when(!highlighted, |this| this.hover(|style| style.bg(p.sunken)))
            // 当前播放歌曲添加左侧品牌色强调条
            .when(current, |this| {
                this.border_l_4().border_color(p.brand)
            })
            .cursor_pointer();
        let position_indicator = if multi_selected {
            div()
                .size(px(22.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(tokens::radius::CONTROL))
                .bg(p.brand)
                .child(
                    Icon::new(KitIcon::Check)
                        .with_size(px(13.0))
                        .text_color(p.on_brand),
                )
                .into_any_element()
        } else if queue_drawer && current {
            self.render_playing_indicator(&key, true, animated, cx)
        } else {
            div()
                .w_full()
                .text_size(px(tokens::CAPTION))
                .text_center()
                .font_weight(if keyboard_selected {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if keyboard_selected || current {
                    p.brand
                } else {
                    p.text_muted
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
        .when(scope.shows_row_cover(), |row| {
            row.child(
                div()
                    .w(px(42.0))
                    .h(px(42.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(self.render_cover_styled(
                        song.cover_art.as_deref(),
                        40.0,
                        tokens::radius::CONTROL,
                        Vec::new(),
                    )),
            )
        })
        .child(
            h_flex()
                .flex_1()
                .flex_grow(3.0)
                .min_w_0()
                .gap_2()
                .text_color(if current { p.brand } else { p.text })
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
                                        .text_size(px(tokens::BODY))
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
                                            .bg(p.brand_soft)
                                            .text_size(px(tokens::CAPTION))
                                            .text_color(p.brand)
                                            .child(label),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .text_size(px(tokens::META))
                                .text_color(p.text_secondary)
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
                    .text_size(px(tokens::META))
                    .text_color(p.text_secondary)
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
                .text_size(px(tokens::META))
                .text_right()
                .text_color(p.text_secondary)
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
                                this.move_queue_row(target, -1);
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
                                this.move_queue_row(target, 1);
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
                            .text_color(p.text_secondary)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.remove_queue_row(target);
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
                            .text_color(p.brand)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.click_song_row(target, SongRowClickAction::Play);
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
                                            let Some((scope, index)) = target.resolve(&this.state)
                                            else {
                                                return menu;
                                            };
                                            let Some(context) =
                                                this.song_menu_context(scope, index)
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
                this.click_song_row(
                    target,
                    song_row_click_action(event.modifiers(), event.click_count()),
                );
                cx.notify();
            }),
        )
        .context_menu(move |menu, _, cx| {
            menu_app
                .update(cx, |this, cx| {
                    let Some((scope, index)) = target.resolve(&this.state) else {
                        return menu;
                    };
                    let context = this.song_menu_context(scope, index);
                    let Some(context) = context else {
                        return menu;
                    };
                    this.song_menu(menu, context, cx)
                })
                .unwrap_or_else(|_| unreachable!("app outlives its rows"))
        })
        .into_any_element()
    }

    pub(in crate::app) fn render_playing_indicator(
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
                    .text_color(palette(cx).text_muted)
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
                    .bg(palette(cx).brand);

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
}
