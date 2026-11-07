//! Album artwork cards, playback actions and metadata links.

use crate::app::prelude::*;
use crate::app::shell::view_helpers::item_instance_key;
use crate::app::widgets::shelf::cover_grid;

impl NavidromeApp {
    pub(in crate::app) fn render_album_grid(
        &self,
        scope: &str,
        albums: &[Album],
        cx: &Context<Self>,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        let grid_start = std::time::Instant::now();
        let columns = self.content_columns(window);
        let cards = albums
            .iter()
            .enumerate()
            .map(|(index, album)| {
                self.render_album_card(scope, index, album, columns.width, cx, window)
            })
            .collect::<Vec<_>>();
        let grid = cover_grid(columns, cards);
        let grid_elapsed = grid_start.elapsed();
        if grid_elapsed > Duration::from_millis(8) {
            log::warn!(
                "render_album_grid built {} albums in {grid_elapsed:?}",
                albums.len()
            );
        }
        grid.into_any_element()
    }

    fn render_album_card(
        &self,
        scope: &str,
        index: usize,
        album: &Album,
        width: f32,
        cx: &Context<Self>,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let key = item_instance_key(scope, "album", index, &album.id);
        let hovered = self.state.hovered_item.as_deref() == Some(key.as_str());
        let album_for_click = album.clone();
        let album_for_play = album.clone();
        let album_for_cover = album.clone();
        let album_id_for_hover = key.clone();
        v_flex()
            .id(SharedString::from(key))
            .min_w_0()
            .gap_1()
            .child(
                div()
                    .id(SharedString::from(format!("album-cover-{}", album.id)))
                    .relative()
                    .w(px(width))
                    .h(px(width))
                    .mb_1()
                    .cursor_pointer()
                    .on_hover(cx.listener(move |this, hovering, _, cx| {
                        this.set_hovered(&album_id_for_hover, *hovering, cx);
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_album(album_for_cover.clone());
                        cx.notify();
                    }))
                    .child(self.render_cover_styled(
                        album.cover_art.as_deref(),
                        width,
                        tokens::radius::COVER,
                        cover_shadow(hovered, cx),
                    ))
                    .child(
                        div()
                            .absolute()
                            .top_2()
                            .right_2()
                            .rounded_full()
                            .bg(p.raised.opacity(0.72))
                            .child(self.favorite_button(FavoriteKind::Album, &album.id, cx)),
                    )
                    .when(hovered, |this| {
                        this.child(
                            div()
                                .absolute()
                                .right_2()
                                .bottom_2()
                                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                .child(
                                    round_play_button(
                                        SharedString::from(format!("album-play-{}", album.id)),
                                        PlayerIcon::Play,
                                        t!("Play album"),
                                        tokens::HOVER_PLAY_BUTTON,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.play_album(album_for_play.clone());
                                            cx.notify();
                                        },
                                    )),
                                ),
                        )
                    }),
            )
            .child(
                div()
                    .id(SharedString::from(format!("album-{}", album.id)))
                    .min_w_0()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_album(album_for_click.clone());
                        cx.notify();
                    }))
                    .child(self.render_scrolling_title(
                        album.name.clone().into(),
                        width,
                        SharedString::from(format!("album-title-{}", album.id)),
                        window,
                    )),
            )
            .child(
                h_flex()
                    .text_size(px(tokens::META))
                    .text_color(p.text_secondary)
                    .min_w_0()
                    .gap_1()
                    .child(self.artist_name_link(
                        SharedString::from(format!("album-grid-artist-{}", album.id)),
                        album.artist.clone(),
                        album.artist_id.clone(),
                        album.cover_art.clone(),
                        cx,
                    ))
                    .when_some(album.year, |this, year| {
                        this.child(t!("- %{year}", year = year))
                    }),
            )
            .into_any_element()
    }
}
