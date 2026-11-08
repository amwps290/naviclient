//! 播放列表集合页、空状态及创建和导入入口渲染。

use crate::app::prelude::*;
use crate::app::widgets::shelf::cover_grid;

impl NavidromeApp {
    pub(in crate::app) fn render_playlists(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let bilibili = self.browsing_bilibili();
        let columns = self.content_columns(window);
        let p = palette(cx);
        let playlist_list = cover_grid(
            columns,
            self.state
                .playlists
                .visible_items()
                .iter()
                .map(|playlist| {
                    let playlist_for_click = playlist.clone();
                    let shared_owner =
                        (!crate::playlist::can_edit(playlist, &self.config.username))
                            .then_some(playlist.owner.as_deref())
                            .flatten();
                    let details = if bilibili {
                        t!(
                            "%{count} videos",
                            count = playlist.song_count.unwrap_or_default()
                        )
                        .to_string()
                    } else {
                        playlist_card_subtitle(
                            playlist.song_count.unwrap_or_default(),
                            playlist.duration,
                            shared_owner,
                        )
                    };
                    v_flex()
                        .id(SharedString::from(format!("playlist-row-{}", playlist.id)))
                        .min_w_0()
                        .gap_1()
                        .cursor_pointer()
                        .child(div().mb_1().child(self.render_cover_styled(
                            playlist.cover_art.as_deref(),
                            columns.width,
                            tokens::radius::COVER,
                            cover_shadow(false, cx),
                        )))
                        .child(
                            div()
                                .text_size(px(tokens::BODY))
                                .font_weight(FontWeight::MEDIUM)
                                .truncate()
                                .child(playlist.name.clone()),
                        )
                        .child(
                            div()
                                .text_size(px(tokens::META))
                                .text_color(p.text_secondary)
                                .truncate()
                                .child(details),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_playlist(playlist_for_click.clone());
                            cx.notify();
                        }))
                        .into_any_element()
                })
                .collect::<Vec<_>>(),
        );

        v_flex()
            .gap_6()
            .child(
                h_flex()
                    .w_full()
                    .items_end()
                    .gap_3()
                    .flex_wrap()
                    .child(div().flex_1().min_w(px(240.0)).child(self.page_header(
                        if bilibili { t!("My Bilibili favorites") } else { t!("Playlists") },
                        if bilibili {
                            t!("%{count} favorite folders", count = self.state.playlists.items().len())
                        } else {
                            t!("%{count} playlists", count = self.state.playlists.items().len())
                        },
                        cx,
                    )))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1()
                            .when(!bilibili, |this| this.child(
                                Button::new("new-playlist")
                                    .label(t!("New playlist"))
                                    .primary()
                                    .small()
                                    .disabled(self.state.playlists.editing() || self.api.is_none())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.name_playlist(None, vec![], window, cx)
                                    })),
                            ))
                            .when(!bilibili, |this| this.child(
                                Button::new("import-playlist")
                                    .label(t!("Import playlist"))
                                    .outline()
                                    .small()
                                    .disabled(self.api.is_none() || self.state.playlists.editing())
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_import_dialog(window, cx);
                                    })),
                            ))
                            .child(self.header_refresh_button(
                                "refresh-playlists",
                                if bilibili { t!("Refresh favorites") } else { t!("Refresh playlists") },
                                self.state.playlists.list_loading(),
                                cx,
                                |this, _cx| this.load_playlists(),
                            )),
                    ),
            )
            .when(
                self.state.playlists.items().is_empty() && self.state.playlists.list_loading(),
                |this| this.child(self.render_album_skeleton_grid(window, cx)),
            )
            .when_some(self.state.playlists.list_error(), |this, error| {
                this.child(
                    div().text_sm().text_color(palette(cx).text_secondary).child(error.to_string()),
                )
            })
            .when(
                self.state.playlists.items().is_empty()
                    && !self.state.playlists.list_loading()
                    && self.state.playlists.list_error().is_none(),
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
                                    .child(if bilibili { t!("No Bilibili favorites yet") } else { t!("No playlists are available.") }),
                            )
                            .child(
                                div()
                                    .max_w(px(420.0))
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if bilibili { t!("Save videos to your favorites in Bilibili, then refresh here.") } else { t!("Create a playlist or import one to organize your music.") }),
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

fn playlist_card_subtitle(
    track_count: i32,
    duration: Option<i32>,
    shared_owner: Option<&str>,
) -> String {
    let mut parts = vec![t!("%{track_count} tracks", track_count = track_count).to_string()];
    if duration.is_some() {
        parts.push(format_duration(duration));
    }
    if let Some(owner) = shared_owner {
        parts.push(t!("Shared").to_string());
        parts.push(owner.to_string());
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests;
