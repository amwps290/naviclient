//! 艺术家、专辑与播放列表详情页渲染。

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn render_artist_detail(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(artist) = self.state.artist_details.artist() else {
            return div().child(t!("Artist not found")).into_any_element();
        };
        let popular_count = self.state.artist_details.top_songs().len();
        let popular_disabled = self.state.artist_details.top_songs_loading() || popular_count == 0;
        let artist_for_play_all = artist.clone();
        let artist_radio_loading = self.artist_radio_loading(&artist.id);
        v_flex()
            .gap_5()
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .items_end()
                    .flex_wrap()
                    .child(self.render_cover(artist.cover_art.as_deref(), 112.0, cx))
                    .child(div().flex_1().min_w(px(220.0)).child(self.page_header(
                        artist.name.clone(),
                        t!(
                            "%{albums} albums · %{songs} songs",
                            albums = self.state.artist_details.albums().len(),
                            songs = popular_count
                        ),
                        cx,
                    )))
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                Button::new("play-artist-all")
                                    .label(t!("Play all"))
                                    .primary()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.play_artist(artist_for_play_all.clone());
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("play-artist-popular")
                                    .label(t!("Play popular"))
                                    .outline()
                                    .disabled(popular_disabled)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.play_song_at_scope(SongScope::ArtistTop, 0);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("start-artist-radio")
                                    .label(if artist_radio_loading {
                                        t!("Building artist radio…")
                                    } else {
                                        t!("Start artist radio")
                                    })
                                    .outline()
                                    .disabled(popular_disabled || artist_radio_loading)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.start_artist_radio();
                                        cx.notify();
                                    })),
                            )
                            .child(self.favorite_button(FavoriteKind::Artist, &artist.id, cx)),
                    ),
            )
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t!("Popular songs")),
            )
            .child(if self.state.artist_details.top_songs_loading() {
                h_flex()
                    .h(px(96.0))
                    .w_full()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("Loading popular songs…"))
                    .into_any_element()
            } else if let Some(error) = self.state.artist_details.top_songs_error() {
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().border)
                    .p_3()
                    .child(
                        div()
                            .flex_1()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("Popular songs are unavailable: %{error}", error = error)),
                    )
                    .child(
                        Button::new("retry-artist-popular")
                            .icon(AppIcon::Refresh)
                            .label(t!("Retry"))
                            .small()
                            .outline()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.load_artist_top_songs();
                                cx.notify();
                            })),
                    )
                    .into_any_element()
            } else if self.state.artist_details.top_songs().is_empty() {
                div()
                    .py_4()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t!("No popular songs were returned for this artist."))
                    .into_any_element()
            } else {
                self.render_song_list(SongScope::ArtistTop, cx)
            })
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t!("Albums")),
            )
            .when(self.state.artist_details.albums_error().is_some(), |this| {
                this.child(
                    self.render_retry_row("retry-artist-albums", cx, |this, cx| {
                        this.load_artist_albums();
                        cx.notify();
                    }),
                )
            })
            .child(if self.state.artist_details.albums_loading() {
                self.render_album_skeleton_grid(cx)
            } else {
                self.render_album_grid(
                    "artist-detail",
                    self.state.artist_details.albums(),
                    cx,
                    window,
                )
            })
            .into_any_element()
    }

    pub(in crate::app) fn render_album_detail(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let Some(album) = self.state.album_details.album() else {
            return div().child(t!("Album not found")).into_any_element();
        };
        let actions_disabled =
            self.state.album_details.loading() || self.state.album_details.songs().is_empty();
        v_flex()
            .h_full()
            .min_h_0()
            .gap_5()
            .child(
                h_flex()
                    .gap_5()
                    .items_end()
                    .flex_none()
                    .child(self.render_cover(album.cover_art.as_deref(), 180.0, cx))
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(album.name.clone()),
                            )
                            .child(
                                h_flex()
                                    .gap_1()
                                    .text_color(cx.theme().muted_foreground)
                                    .min_w_0()
                                    .child(self.artist_name_link(
                                        "album-detail-artist",
                                        album.artist.clone(),
                                        album.artist_id.clone(),
                                        album.cover_art.clone(),
                                        cx,
                                    ))
                                    .when_some(album.year, |this, year| {
                                        this.child(t!("- %{year}", year = year))
                                    }),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .flex_wrap()
                                    .child(
                                        Button::new("play-album")
                                            .label(t!("Play album"))
                                            .primary()
                                            .disabled(actions_disabled)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.play_song_at_scope(SongScope::AlbumDetail, 0);
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new("append-album")
                                            .icon(AppIcon::Queue)
                                            .label(t!("Append all"))
                                            .outline()
                                            .disabled(actions_disabled)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.append_songs_at_scope(SongScope::AlbumDetail);
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new("album-add-to-playlist")
                                            .label(t!("Add to playlist…"))
                                            .outline()
                                            .disabled(
                                                self.state.playlists.editing() || actions_disabled,
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.choose_playlist(
                                                    crate::playlist::song_ids(
                                                        this.state.album_details.songs(),
                                                    ),
                                                    window,
                                                    cx,
                                                );
                                            })),
                                    ),
                            ),
                    ),
            )
            .when(self.state.album_details.failed(), |this| {
                this.child(
                    self.render_retry_row("retry-album-details", cx, |this, cx| {
                        this.load_album_details();
                        cx.notify();
                    }),
                )
            })
            .child(if self.state.album_details.loading() {
                self.render_list_skeleton(6, cx)
            } else {
                self.render_song_list(SongScope::AlbumDetail, cx)
            })
            .into_any_element()
    }

    pub(in crate::app) fn render_playlist_detail(
        &self,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(playlist) = self.state.playlists.playlist() else {
            return div().child(t!("Playlist not found")).into_any_element();
        };
        let rename = playlist.clone();
        let delete = playlist.clone();
        let read_only = !crate::playlist::can_edit(playlist, &self.config.username);
        let edit_disabled =
            read_only || self.state.playlists.editing() || self.state.playlists.detail_loading();
        v_flex()
            .h_full()
            .min_h_0()
            .gap_5()
            .child(self.page_header(
                playlist.name.clone(),
                t!(
                    "%{count} tracks",
                    count = self.state.playlists.songs().len()
                ),
                cx,
            ))
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new("play-playlist")
                            .icon(PlayerIcon::Play)
                            .label(t!("Play all"))
                            .small()
                            .info()
                            .outline()
                            .disabled(
                                self.state.playlists.songs().is_empty()
                                    || self.state.playlists.detail_loading(),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.play_song_at_scope(SongScope::PlaylistDetail, 0);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("append-playlist")
                            .icon(AppIcon::Queue)
                            .label(t!("Append all"))
                            .small()
                            .outline()
                            .disabled(
                                self.state.playlists.songs().is_empty()
                                    || self.state.playlists.detail_loading(),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.append_songs_at_scope(SongScope::PlaylistDetail);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("rename-playlist")
                            .label(t!("Rename"))
                            .small()
                            .outline()
                            .disabled(edit_disabled)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.name_playlist(Some(rename.clone()), vec![], window, cx)
                            })),
                    )
                    .child(
                        Button::new("delete-playlist")
                            .label(t!("Delete"))
                            .small()
                            .ghost()
                            .disabled(edit_disabled)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.delete_playlist(delete.clone(), window, cx)
                            })),
                    )
                    .child(
                        Button::new("refresh-playlist")
                            .icon(AppIcon::Refresh)
                            .tooltip(t!("Refresh playlist"))
                            .small()
                            .ghost()
                            .disabled(
                                self.state.playlists.editing()
                                    || self.state.playlists.detail_loading(),
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.load_playlist_detail();
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.state.playlists.editing() {
                        t!("Saving playlist…")
                    } else if self.state.playlists.detail_loading() {
                        t!("Loading playlist…")
                    } else if read_only {
                        t!("Shared playlist · Only the owner can edit it.")
                    } else if self.state.playlists.songs().is_empty() {
                        t!("This playlist is empty. Use a song’s actions menu to add tracks.")
                    } else {
                        t!("Use a song’s actions menu to move or remove it.")
                    }),
            )
            .child(self.render_song_list(SongScope::PlaylistDetail, cx))
            .into_any_element()
    }
}
