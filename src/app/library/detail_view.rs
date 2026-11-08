//! 艺术家、专辑与播放列表详情页渲染。

use crate::app::prelude::*;
use crate::app::widgets::detail_hero::DetailHero;
use crate::app::widgets::shelf::shelf_section;

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
        let padding = self.shell_layout(window).page_padding;
        let p = palette(cx);
        let hero = self.render_detail_hero(
            DetailHero {
                kind: t!("Artist").into(),
                title: artist.name.clone().into(),
                cover_id: artist.cover_art.clone(),
                round_cover: true,
                meta: div()
                    .child(t!(
                        "%{albums} albums · %{songs} songs",
                        albums = self.state.artist_details.albums().len(),
                        songs = popular_count
                    ))
                    .into_any_element(),
                actions: h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(
                        round_play_button(
                            "play-artist-all",
                            PlayerIcon::Play,
                            t!("Play all"),
                            tokens::DETAIL_PLAY_BUTTON,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.play_artist(artist_for_play_all.clone());
                            cx.notify();
                        })),
                    )
                    .child(
                        capsule_button("play-artist-popular")
                            .label(t!("Play popular"))
                            .disabled(popular_disabled)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.play_song_at_scope(SongScope::ArtistTop, 0);
                                cx.notify();
                            })),
                    )
                    .child(
                        capsule_button("start-artist-radio")
                            .label(if artist_radio_loading {
                                t!("Building artist radio…")
                            } else {
                                t!("Start artist radio")
                            })
                            .disabled(popular_disabled || artist_radio_loading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.start_artist_radio();
                                cx.notify();
                            })),
                    )
                    .child(self.favorite_button(FavoriteKind::Artist, &artist.id, cx))
                    .into_any_element(),
            },
            window,
            cx,
        );
        let popular: gpui_kit::AnyElement = if self.state.artist_details.top_songs_loading() {
            h_flex()
                .h(px(96.0))
                .w_full()
                .items_center()
                .justify_center()
                .text_size(px(tokens::BODY))
                .text_color(p.text_secondary)
                .child(t!("Loading popular songs…"))
                .into_any_element()
        } else if let Some(error) = self.state.artist_details.top_songs_error() {
            h_flex()
                .w_full()
                .gap_3()
                .items_center()
                .rounded(px(tokens::radius::COVER))
                .bg(p.sunken)
                .p_3()
                .child(
                    div()
                        .flex_1()
                        .text_size(px(tokens::BODY))
                        .text_color(p.text_secondary)
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
                .text_size(px(tokens::BODY))
                .text_color(p.text_secondary)
                .child(t!("No popular songs were returned for this artist."))
                .into_any_element()
        } else {
            self.render_song_list(SongScope::ArtistTop, cx)
        };
        let albums = if self.state.artist_details.albums_loading() {
            self.render_album_skeleton_grid(window, cx)
        } else {
            self.render_album_grid(
                "artist-detail",
                self.state.artist_details.albums(),
                cx,
                window,
            )
        };
        v_flex()
            .child(hero)
            .child(
                v_flex()
                    .px(px(padding))
                    .pb(px(padding))
                    .gap(px(tokens::SECTION_GAP))
                    .child(shelf_section(
                        section_title(t!("Popular songs"), cx).into_any_element(),
                        popular,
                    ))
                    .child(shelf_section(
                        section_title(t!("Albums"), cx).into_any_element(),
                        v_flex()
                            .gap_3()
                            .when(self.state.artist_details.albums_error().is_some(), |this| {
                                this.child(self.render_retry_row(
                                    "retry-artist-albums",
                                    cx,
                                    |this, cx| {
                                        this.load_artist_albums();
                                        cx.notify();
                                    },
                                ))
                            })
                            .child(albums),
                    )),
            )
            .into_any_element()
    }

    pub(in crate::app) fn render_album_detail(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(album) = self.state.album_details.album() else {
            return div().child(t!("Album not found")).into_any_element();
        };
        let actions_disabled =
            self.state.album_details.loading() || self.state.album_details.songs().is_empty();
        let padding = self.shell_layout(window).page_padding;
        let hero = self.render_detail_hero(
            DetailHero {
                kind: t!("Album").into(),
                title: album.name.clone().into(),
                cover_id: album.cover_art.clone(),
                round_cover: false,
                meta: h_flex()
                    .gap_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_color(palette(cx).brand)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.artist_name_link(
                                "album-detail-artist",
                                album.artist.clone(),
                                album.artist_id.clone(),
                                album.cover_art.clone(),
                                cx,
                            )),
                    )
                    .when_some(album.year, |this, year| {
                        this.child(t!("- %{year}", year = year))
                    })
                    .into_any_element(),
                actions: h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(
                        round_play_button(
                            "play-album",
                            PlayerIcon::Play,
                            t!("Play album"),
                            tokens::DETAIL_PLAY_BUTTON,
                        )
                        .disabled(actions_disabled)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.play_song_at_scope(SongScope::AlbumDetail, 0);
                            cx.notify();
                        })),
                    )
                    .child(
                        capsule_button("append-album")
                            .icon(AppIcon::Queue)
                            .label(t!("Append all"))
                            .disabled(actions_disabled)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.append_songs_at_scope(SongScope::AlbumDetail);
                                cx.notify();
                            })),
                    )
                    .child(
                        capsule_button("album-add-to-playlist")
                            .label(t!("Add to playlist…"))
                            .disabled(self.state.playlists.editing() || actions_disabled)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.choose_playlist(
                                    crate::playlist::song_ids(this.state.album_details.songs()),
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .into_any_element(),
            },
            window,
            cx,
        );
        v_flex()
            .h_full()
            .min_h_0()
            .child(hero)
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .px(px(padding))
                    .pb(px(padding))
                    .gap_3()
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
                    }),
            )
            .into_any_element()
    }

    pub(in crate::app) fn render_playlist_detail(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(playlist) = self.state.playlists.playlist() else {
            return div().child(t!("Playlist not found")).into_any_element();
        };
        let rename = playlist.clone();
        let delete = playlist.clone();
        let bilibili = self.browsing_bilibili();
        let read_only = bilibili || !crate::playlist::can_edit(playlist, &self.config.username);
        let edit_disabled =
            read_only || self.state.playlists.editing() || self.state.playlists.detail_loading();
        let playback_disabled =
            self.state.playlists.songs().is_empty() || self.state.playlists.detail_loading();
        let padding = self.shell_layout(window).page_padding;
        let hero = self.render_detail_hero(
            DetailHero {
                kind: if bilibili {
                    t!("Bilibili favorite folder")
                } else {
                    t!("Playlist")
                }
                .into(),
                title: playlist.name.clone().into(),
                cover_id: playlist.cover_art.clone(),
                round_cover: false,
                meta: div()
                    .child(t!(
                        "%{count} tracks",
                        count = self.state.playlists.songs().len()
                    ))
                    .into_any_element(),
                actions: h_flex()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(
                        round_play_button(
                            "play-playlist",
                            PlayerIcon::Play,
                            t!("Play all"),
                            tokens::DETAIL_PLAY_BUTTON,
                        )
                        .disabled(playback_disabled)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.play_song_at_scope(SongScope::PlaylistDetail, 0);
                            cx.notify();
                        })),
                    )
                    .child(
                        capsule_button("append-playlist")
                            .icon(AppIcon::Queue)
                            .label(t!("Append all"))
                            .disabled(playback_disabled)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.append_songs_at_scope(SongScope::PlaylistDetail);
                                cx.notify();
                            })),
                    )
                    .when(!bilibili, |this| {
                        this.child(
                            capsule_button("rename-playlist")
                                .label(t!("Rename"))
                                .disabled(edit_disabled)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.name_playlist(Some(rename.clone()), vec![], window, cx)
                                })),
                        )
                    })
                    .when(!bilibili, |this| {
                        this.child(
                            capsule_button("delete-playlist")
                                .label(t!("Delete"))
                                .disabled(edit_disabled)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.delete_playlist(delete.clone(), window, cx)
                                })),
                        )
                    })
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
                    )
                    .into_any_element(),
            },
            window,
            cx,
        );
        v_flex()
            .h_full()
            .min_h_0()
            .child(hero)
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .px(px(padding))
                    .pb(px(padding))
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(tokens::META))
                            .text_color(palette(cx).text_muted)
                            .child(if self.state.playlists.editing() {
                                t!("Saving playlist…")
                            } else if self.state.playlists.detail_loading() {
                                if bilibili { t!("Loading all favorite pages and parts…") } else { t!("Loading playlist…") }
                            } else if let Some(error) = self.state.playlists.detail_error() {
                                std::borrow::Cow::Owned(error.to_string())
                            } else if bilibili {
                                t!("All pages loaded · Video order and part order preserved")
                            } else if read_only {
                                t!("Shared playlist · Only the owner can edit it.")
                            } else if self.state.playlists.songs().is_empty() {
                                t!("This playlist is empty. Use a song’s actions menu to add tracks.")
                            } else {
                                t!("Use a song’s actions menu to move or remove it.")
                            }),
                    )
                    .child(self.render_song_list(SongScope::PlaylistDetail, cx)),
            )
            .into_any_element()
    }
}
