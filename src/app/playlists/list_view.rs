//! 播放列表集合页、空状态及创建和导入入口渲染。

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn render_playlists(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let playlist_list = v_flex()
            .w_full()
            .border_t_1()
            .border_b_1()
            .border_color(cx.theme().border.opacity(0.72))
            .children(self.state.playlists.visible_items().iter().enumerate().map(
                |(index, playlist)| {
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
                        .unwrap_or_else(|| t!("%{track_count} tracks", track_count = track_count));

                    h_flex()
                        .id(SharedString::from(format!("playlist-row-{}", playlist.id)))
                        .h(px(68.0))
                        .px_3()
                        .gap_3()
                        .items_center()
                        .cursor_pointer()
                        .when(index > 0, |this| {
                            this.border_t_1()
                                .border_color(cx.theme().border.opacity(0.52))
                        })
                        .hover(|style| style.bg(cx.theme().list_hover))
                        .child(self.render_cover(playlist.cover_art.as_deref(), 48.0, cx))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
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
                },
            ));

        v_flex()
            .gap_5()
            .child(
                h_flex()
                    .w_full()
                    .items_end()
                    .gap_3()
                    .flex_wrap()
                    .child(div().flex_1().min_w(px(240.0)).child(self.page_header(
                        t!("Playlists"),
                        t!(
                            "%{count} playlists",
                            count = self.state.playlists.items().len()
                        ),
                        cx,
                    )))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .child(
                                Button::new("new-playlist")
                                    .label(t!("New playlist"))
                                    .primary()
                                    .small()
                                    .disabled(self.state.playlists.editing() || self.api.is_none())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.name_playlist(None, vec![], window, cx)
                                    })),
                            )
                            .child(
                                Button::new("import-playlist")
                                    .label(t!("Import playlist"))
                                    .outline()
                                    .small()
                                    .disabled(self.api.is_none() || self.state.playlists.editing())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_import_dialog(window, cx);
                                    })),
                            )
                            .child(self.header_refresh_button(
                                "refresh-playlists",
                                t!("Refresh playlists"),
                                self.state.playlists.list_loading(),
                                cx,
                                |this, _cx| this.load_playlists(),
                            )),
                    ),
            )
            .when(
                self.state.playlists.items().is_empty() && self.state.playlists.list_loading(),
                |this| this.child(self.render_list_skeleton(8, cx)),
            )
            .when(
                self.state.playlists.items().is_empty() && !self.state.playlists.list_loading(),
                |this| {
                    this.child(
                        v_flex()
                            .w_full()
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
                                    .child(t!("No playlists are available.")),
                            )
                            .child(
                                div()
                                    .max_w(px(420.0))
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!(
                                        "Create a playlist or import one to organize your music."
                                    )),
                            ),
                    )
                },
            )
            .when(!self.state.playlists.items().is_empty(), |this| {
                this.child(playlist_list)
            })
            .into_any_element()
    }
}
