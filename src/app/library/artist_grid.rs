//! Artist artwork cards and library actions.

use crate::app::prelude::*;
use crate::app::shell::view_helpers::item_instance_key;
use crate::app::widgets::shelf::cover_grid;

impl NavidromeApp {
    pub(in crate::app) fn render_artist_grid(
        &self,
        artists: &[Artist],
        cx: &Context<Self>,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        let columns = self.content_columns(window);
        let p = palette(cx);
        let cards =
            artists
                .iter()
                .enumerate()
                .map(|(index, artist)| {
                    let width = columns.width;
                    let key = item_instance_key("artists", "artist", index, &artist.id);
                    let hovered = self.state.hovered_item.as_deref() == Some(key.as_str());
                    let artist_for_click = artist.clone();
                    let artist_for_play = artist.clone();
                    let artist_for_cover = artist.clone();
                    let artist_id_for_hover = key.clone();
                    v_flex()
                        .id(SharedString::from(key))
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .id(SharedString::from(format!("artist-cover-{}", artist.id)))
                                .relative()
                                .w(px(width))
                                .h(px(width))
                                .mb_1()
                                .cursor_pointer()
                                .on_hover(cx.listener(move |this, hovering, _, cx| {
                                    this.set_hovered(&artist_id_for_hover, *hovering, cx);
                                }))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_artist(artist_for_cover.clone());
                                    cx.notify();
                                }))
                                .child(self.render_cover_styled(
                                    artist.cover_art.as_deref(),
                                    width,
                                    tokens::radius::FULL,
                                    cover_shadow(hovered, cx),
                                ))
                                .child(
                                    div()
                                        .absolute()
                                        .top_2()
                                        .right_2()
                                        .rounded_full()
                                        .bg(p.raised.opacity(0.72))
                                        .child(self.favorite_button(
                                            FavoriteKind::Artist,
                                            &artist.id,
                                            cx,
                                        )),
                                )
                                .when(hovered, |this| {
                                    this.child(
                                        div()
                                            .absolute()
                                            .right_2()
                                            .bottom_2()
                                            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                                cx.stop_propagation()
                                            })
                                            .child(
                                                round_play_button(
                                                    SharedString::from(format!(
                                                        "artist-play-{}",
                                                        artist.id
                                                    )),
                                                    PlayerIcon::Play,
                                                    t!("Play all"),
                                                    tokens::HOVER_PLAY_BUTTON,
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    cx.stop_propagation();
                                                    this.play_artist(artist_for_play.clone());
                                                    cx.notify();
                                                })),
                                            ),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .id(SharedString::from(format!("artist-{}", artist.id)))
                                .min_w_0()
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_artist(artist_for_click.clone());
                                    cx.notify();
                                }))
                                .child(self.render_scrolling_title(
                                    artist.name.clone().into(),
                                    width,
                                    SharedString::from(format!("artist-title-{}", artist.id)),
                                    window,
                                )),
                        )
                        .child(
                            div()
                                .text_size(px(tokens::META))
                                .text_color(p.text_secondary)
                                .child(
                                    artist
                                        .album_count
                                        .map(|count| t!("%{count} albums", count = count))
                                        .unwrap_or_else(|| t!("Artist")),
                                ),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>();
        cover_grid(columns, cards).into_any_element()
    }
}
