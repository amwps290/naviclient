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
        let layout = self.shell_layout(window);
        let padding = layout.page_padding;
        let player_clearance = layout.player_clearance();
        let p = palette(cx);
        let hero = self.render_detail_hero(
            DetailHero {
                kind: t!("Artist").into(),
                title: artist.name.clone().into(),
                cover_id: artist.cover_art.clone(),
                round_cover: true,
                wide_cover: false,
                meta: div()
                    .child(t!(
                        "%{albums} albums",
                        albums = self.state.artist_details.albums().len()
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
                            cx,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.play_artist(artist_for_play_all.clone());
                            cx.notify();
                        })),
                    )
                    .child(
                        capsule_button("play-artist-popular")
                            .label(t!("Play popular"))
                            .disabled_tonal(popular_disabled)
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
                            .disabled_tonal(popular_disabled || artist_radio_loading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.start_artist_radio();
                                cx.notify();
                            })),
                    )
                    .child(self.favorite_button_sized(
                        FavoriteKind::Artist,
                        &artist.id,
                        tokens::ICON_BUTTON_LG,
                        cx,
                    ))
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
                    surface_button("retry-artist-popular")
                        .icon(AppIcon::Refresh)
                        .label(t!("Retry"))
                        .compact_action()
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
                    .pb(px(padding + player_clearance))
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
        let songs = self.state.album_details.songs();
        let actions_disabled = self.state.album_details.loading() || songs.is_empty();
        let track_count = if !songs.is_empty() {
            Some(songs.len())
        } else {
            album.song_count.filter(|c| *c > 0).map(|c| c as usize)
        };
        let total_duration = album.duration.filter(|d| *d > 0).or_else(|| {
            let sum: i32 = songs
                .iter()
                .filter_map(|song| song.duration.filter(|d| *d > 0))
                .sum();
            (sum > 0).then_some(sum)
        });
        let uniform_format = songs.first().and_then(|first| {
            let suffix = first.suffix.as_deref().filter(|s| !s.is_empty())?;
            songs
                .iter()
                .all(|song| {
                    song.suffix
                        .as_deref()
                        .is_some_and(|other| other.eq_ignore_ascii_case(suffix))
                })
                .then(|| suffix.to_uppercase())
        });
        let genre = album
            .genre
            .as_deref()
            .map(str::trim)
            .filter(|genre| !genre.is_empty())
            .map(str::to_string);
        let layout = self.shell_layout(window);
        let padding = layout.page_padding;
        let player_clearance = layout.player_clearance();
        let p = palette(cx);
        let hero = self.render_detail_hero(
            DetailHero {
                kind: t!("Album").into(),
                title: album.name.clone().into(),
                cover_id: album.cover_art.clone(),
                round_cover: false,
                wide_cover: false,
                meta: h_flex()
                    .gap_1p5()
                    .flex_wrap()
                    .items_center()
                    .min_w_0()
                    .child(
                        div()
                            .text_color(p.brand)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.artist_name_link(
                                "album-detail-artist",
                                album.artist.clone(),
                                album.artist_id.clone(),
                                album.cover_art.clone(),
                                cx,
                            )),
                    )
                    .when_some(album.year, |this, year| this.child(format!("· {year}")))
                    .when_some(track_count, |this, count| {
                        this.child(format!("· {}", t!("%{count} tracks", count = count)))
                    })
                    .when_some(total_duration, |this, duration| {
                        this.child(format!("· {}", format_duration(Some(duration))))
                    })
                    .when_some(genre, |this, genre| {
                        let genre_click = genre.clone();
                        this.child("·").child(
                            div()
                                .id("album-detail-genre")
                                .cursor_pointer()
                                .px_2()
                                .py_0p5()
                                .rounded_full()
                                .bg(p.sunken)
                                .text_xs()
                                .text_color(p.text_secondary)
                                .hover(move |style| style.text_color(p.brand).bg(p.brand_soft))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_genre(genre_click.clone());
                                    cx.notify();
                                }))
                                .child(genre),
                        )
                    })
                    .when_some(uniform_format, |this, format| {
                        this.child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded_full()
                                .bg(p.brand_soft)
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.brand)
                                .child(format),
                        )
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
                            cx,
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
                            .disabled_tonal(actions_disabled)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.append_songs_at_scope(SongScope::AlbumDetail);
                                cx.notify();
                            })),
                    )
                    .child(
                        capsule_button("album-add-to-playlist")
                            .label(t!("Add to playlist…"))
                            .disabled_tonal(self.state.playlists.editing() || actions_disabled)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.choose_playlist(
                                    crate::playlist::song_ids(this.state.album_details.songs()),
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .when(!self.browsing_local_folder(), |this| {
                        let all_pinned = !songs.is_empty()
                            && songs.iter().all(|song| self.song_is_pinned_offline(song));
                        let any_downloading = songs.iter().any(|song| {
                            self.song_offline_status(song)
                                == crate::offline::OfflineTrackStatus::Downloading
                        });
                        let label = if any_downloading {
                            t!("Downloading…")
                        } else if all_pinned {
                            t!("Saved offline")
                        } else {
                            t!("Download offline")
                        };
                        this.child(
                            capsule_button("album-offline-pin")
                                .icon(AppIcon::Download)
                                .label(label)
                                .disabled_tonal(
                                    actions_disabled
                                        || any_downloading
                                        || (self.offline_mode_active() && !all_pinned),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let tracks = this.state.album_details.songs().to_vec();
                                    if all_pinned {
                                        this.unpin_songs_offline(&tracks);
                                    } else {
                                        this.pin_songs_offline(tracks, None);
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .child(self.favorite_button_sized(
                        FavoriteKind::Album,
                        &album.id,
                        tokens::ICON_BUTTON_LG,
                        cx,
                    ))
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
                    .pb(px(player_clearance))
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
            self.state.playlists.songs().is_empty() || !self.state.playlists.detail_ready();
        let layout = self.shell_layout(window);
        let padding = layout.page_padding;
        let player_clearance = layout.player_clearance();
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
                wide_cover: bilibili,
                meta: div()
                    .child(crate::app::playlists::detail_view::playlist_track_label(
                        &self.state.playlists,
                        bilibili,
                    ))
                    .into_any_element(),
                actions: h_flex()
                    .w_full()
                    .gap_2()
                    .flex_wrap()
                    .items_center()
                    .child(
                        round_play_button(
                            "play-playlist",
                            PlayerIcon::Play,
                            t!("Play all"),
                            tokens::DETAIL_PLAY_BUTTON,
                            cx,
                        )
                        .disabled(playback_disabled)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.play_playlist_all();
                            cx.notify();
                        })),
                    )
                    .child(
                        capsule_button("append-playlist")
                            .icon(AppIcon::Queue)
                            .label(t!("Append all"))
                            .disabled_tonal(playback_disabled)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.append_playlist_all();
                                cx.notify();
                            })),
                    )
                    .when(!self.browsing_local_folder(), |this| {
                        let songs = self.state.playlists.songs();
                        let all_pinned = !songs.is_empty()
                            && songs.iter().all(|song| self.song_is_pinned_offline(song));
                        let any_downloading = songs.iter().any(|song| {
                            self.song_offline_status(song)
                                == crate::offline::OfflineTrackStatus::Downloading
                        });
                        let label = if any_downloading {
                            t!("Downloading…")
                        } else if all_pinned {
                            t!("Saved offline")
                        } else {
                            t!("Download offline")
                        };
                        let pl_snapshot = playlist.clone();
                        this.child(
                            capsule_button("playlist-offline-pin")
                                .icon(AppIcon::Download)
                                .label(label)
                                .disabled_tonal(
                                    playback_disabled
                                        || any_downloading
                                        || (self.offline_mode_active() && !all_pinned),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let tracks = this.state.playlists.songs().to_vec();
                                    if all_pinned {
                                        this.unpin_songs_offline(&tracks);
                                    } else {
                                        this.pin_songs_offline(tracks, Some(pl_snapshot.clone()));
                                    }
                                    cx.notify();
                                })),
                        )
                    })
                    .when(!bilibili, |this| {
                        this.child(
                            ghost_button("rename-playlist")
                                .label(t!("Rename"))
                                .rounded(px(tokens::radius::FULL))
                                .compact_action()
                                .disabled(edit_disabled)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.name_playlist(Some(rename.clone()), vec![], window, cx)
                                })),
                        )
                    })
                    .when(!bilibili, |this| {
                        this.child(
                            ghost_button("delete-playlist")
                                .label(t!("Delete"))
                                .rounded(px(tokens::radius::FULL))
                                .compact_action()
                                .text_color(cx.theme().danger)
                                .disabled(edit_disabled)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.delete_playlist(delete.clone(), window, cx)
                                })),
                        )
                    })
                    .child(
                        icon_button(
                            "refresh-playlist",
                            AppIcon::Refresh,
                            t!("Refresh playlist"),
                            tokens::ICON_BUTTON_MD,
                        )
                        .disabled(
                            self.state.playlists.editing() || self.state.playlists.detail_loading(),
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.load_playlist_detail();
                            cx.notify();
                        })),
                    )
                    .child(self.render_playlist_search(window, cx))
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
                    .pb(px(player_clearance))
                    .gap_3()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_3()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .child(self.render_playlist_status(bilibili, read_only, cx)),
                            )
                            .when(self.state.playlists.search.expanded, |this| {
                                this.child(self.render_playlist_search_feedback(cx))
                            }),
                    )
                    .child(self.render_song_list(SongScope::PlaylistDetail, cx)),
            )
            .into_any_element()
    }
}
