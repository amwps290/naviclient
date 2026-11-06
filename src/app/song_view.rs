//! Virtualized song lists, headers, scroll layout and multi-selection toolbar.

use super::view_helpers::{compact_song_columns, SONG_ROW_HEIGHT};
use super::*;

impl NavidromeApp {
    pub(super) fn render_song_list(
        &self,
        scope: SongScope,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let editable = scope == SongScope::Queue;
        // uniform_list 视口虚拟化：只构建可见行，长列表不再一次性挂载全部元素，
        // 滚动多久元素树就只有多大，与列表总长度无关。
        let Some(songs) = self.state.songs_for_scope(scope) else {
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
        let queue_drawer = scope.is_drawer() && self.state.queue_drawer_open;
        let compact_columns = queue_drawer || compact_song_columns(list_width);
        // 专辑 / 播放列表详情和队列页中表格占满剩余高度（表头固定，列表内部滚动）；
        // 收藏与搜索页的歌曲表格与其他分区共存，限制高度独立滚动。
        let fill = scope.fills_viewport();
        let rows = cx.processor(move |this, range: Range<usize>, _window, cx| {
            let playback = this.audio.state();
            range
                .map(|index| this.render_song_row(scope, index, compact_columns, &playback, cx))
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

    /// 多选操作条：显示所选数量与批量动作。收藏按钮按“是否全部
    /// 已收藏”智能切换方向。所有动作复用单曲操作的批量版本。
    pub(super) fn render_multi_action_bar(
        &self,
        scope: SongScope,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let summary = self.state.selection_summary(scope);
        let favorite_icon = if summary.all_favorited {
            AppIcon::HeartFilled
        } else {
            AppIcon::Heart
        };
        let favorite_label = if summary.all_favorited {
            t!("Unfavorite selected")
        } else {
            t!("Favorite selected")
        };
        let add_disabled = self.state.playlists.editing() || self.api.is_none();
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
                    .child(t!("%{count} selected", count = summary.count)),
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
                        let ids = this.state.selected_song_ids(scope);
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
}
