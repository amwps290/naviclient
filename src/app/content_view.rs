//! 主内容区路由及各媒体库页面的渲染。

use super::*;

impl NavidromeApp {
    pub(super) fn render_content(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        match self.state.view {
            View::Home => self.render_home(window, cx),
            View::Favorites => {
                let favorites = &self.state.favorites;
                let total =
                    favorites.artists.len() + favorites.albums.len() + favorites.songs.len();
                let mut content = v_flex().gap_5().child(
                    h_flex()
                        .w_full()
                        .items_end()
                        .gap_3()
                        .child(div().flex_1().child(self.page_header(
                            t!("Favorites"),
                            t!(
                                "%{total} starred items from your Navidrome library",
                                total = total
                            ),
                            cx,
                        )))
                        .child(self.header_refresh_button(
                            "refresh-favorites",
                            t!("Refresh favorites"),
                            false,
                            cx,
                            |this, _cx| this.load_favorites(),
                        )),
                );

                if total == 0 {
                    content =
                        content.child(div().py_8().text_color(cx.theme().muted_foreground).child(
                            t!("Click a heart on a song, album, or artist to add it here."),
                        ));
                }
                if !favorites.artists.is_empty() {
                    content = content
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(t!("Artists")),
                        )
                        .child(self.render_artist_grid(&favorites.artists, cx, window));
                }
                if !favorites.albums.is_empty() {
                    content = content
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(t!("Albums")),
                        )
                        .child(self.render_album_grid("favorites", &favorites.albums, cx, window));
                }
                if !favorites.songs.is_empty() {
                    content = content
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(t!("Songs")),
                        )
                        .child(self.render_song_list("favorites", false, cx));
                }

                content.into_any_element()
            }
            View::Artists => v_flex()
                .gap_5()
                .child(
                    h_flex()
                        .w_full()
                        .items_end()
                        .gap_3()
                        .child(div().flex_1().child(self.page_header(
                            t!("Artists"),
                            t!(
                                "%{count} artists in your library",
                                count = self.state.artists.len()
                            ),
                            cx,
                        )))
                        .child(self.render_artist_sort_button(cx))
                        .child(self.header_refresh_button(
                            "refresh-artists",
                            t!("Refresh artists"),
                            self.state.loading && self.state.artists.is_empty(),
                            cx,
                            |this, _cx| this.load_artists(),
                        )),
                )
                .child(if self.state.artists.is_empty() && self.state.loading {
                    self.render_artist_skeleton_grid(cx)
                } else {
                    let visible = self.state.artists_visible.min(self.state.artists.len());
                    self.render_artist_grid(&self.state.artists[..visible], cx, window)
                })
                .into_any_element(),
            View::Albums => v_flex()
                .gap_5()
                .child(
                    h_flex()
                        .w_full()
                        .items_end()
                        .gap_3()
                        .child(div().flex_1().child(self.page_header(
                            t!("Albums"),
                            t!(
                                "%{count} albums in your library",
                                count = self.state.albums.len()
                            ),
                            cx,
                        )))
                        .child(self.render_album_sort_button(cx))
                        .child(self.header_refresh_button(
                            "refresh-albums",
                            t!("Refresh albums"),
                            self.state.albums_loading,
                            cx,
                            |this, _cx| this.load_albums(),
                        )),
                )
                .child(
                    if self.state.albums.is_empty() && self.state.albums_loading {
                        self.render_album_skeleton_grid(cx)
                    } else {
                        self.render_album_grid("albums", &self.state.albums, cx, window)
                    },
                )
                .when(
                    self.state.albums_page_failed && !self.state.albums_loading,
                    |this| {
                        this.child(self.render_retry_row("retry-albums", cx, |this, _cx| {
                            this.load_more_albums()
                        }))
                    },
                )
                .into_any_element(),
            View::Genres => {
                // 有旧数据（磁盘缓存）时直接渲染，仅在后台重验证中提示；
                // 首次无缓存才显示骨架屏。
                let subtitle = if self.state.genres_loading && !self.state.genres.is_empty() {
                    t!(
                        "%{count} genres — refreshing…",
                        count = self.state.genres.len()
                    )
                } else {
                    t!(
                        "%{count} genres in your library",
                        count = self.state.genres.len()
                    )
                };
                v_flex()
                    .gap_5()
                    .child(
                        h_flex()
                            .w_full()
                            .items_end()
                            .gap_3()
                            .child(div().flex_1().child(self.page_header(
                                t!("Genres"),
                                subtitle,
                                cx,
                            )))
                            .child(self.header_refresh_button(
                                "refresh-genres",
                                t!("Refresh genres"),
                                self.state.genres_loading,
                                cx,
                                |this, _cx| this.load_genres(),
                            )),
                    )
                    .child(
                        if self.state.genres.is_empty() && self.state.genres_loading {
                            self.render_album_skeleton_grid(cx)
                        } else if self.state.genres.is_empty() {
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("No genres in your library yet."))
                                .into_any_element()
                        } else {
                            self.render_genre_grid(cx)
                        },
                    )
                    .into_any_element()
            }
            View::GenreDetail => {
                let Some(genre) = self.state.current_genre.clone() else {
                    return div().child(t!("Genre not found")).into_any_element();
                };
                v_flex()
                    .gap_5()
                    .child(self.page_header(
                        genre,
                        t!("%{count} albums", count = self.state.genre_albums.len()),
                        cx,
                    ))
                    .child(
                        if self.state.genre_albums.is_empty() && self.state.genre_albums_loading {
                            self.render_album_skeleton_grid(cx)
                        } else if self.state.genre_albums.is_empty() {
                            div()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("No albums in this genre yet."))
                                .into_any_element()
                        } else {
                            self.render_album_grid(
                                "genre-detail",
                                &self.state.genre_albums,
                                cx,
                                window,
                            )
                        },
                    )
                    .when(
                        self.state.genre_albums_page_failed && !self.state.genre_albums_loading,
                        |this| {
                            this.child(self.render_retry_row(
                                "retry-genre-albums",
                                cx,
                                |this, _cx| this.load_more_genre_albums(),
                            ))
                        },
                    )
                    .into_any_element()
            }
            View::Playlists => {
                let visible_playlists =
                    self.state.playlists_visible.min(self.state.playlists.len());
                let playlist_list = v_flex()
                    .w_full()
                    .border_t_1()
                    .border_b_1()
                    .border_color(cx.theme().border.opacity(0.72))
                    .children(
                        self.state.playlists[..visible_playlists]
                            .iter()
                            .enumerate()
                            .map(|(index, playlist)| {
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
                                    .unwrap_or_else(|| {
                                        t!("%{track_count} tracks", track_count = track_count)
                                    });

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
                                    .child(self.render_cover(
                                        playlist.cover_art.as_deref(),
                                        48.0,
                                        cx,
                                    ))
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
                            }),
                    );

                v_flex()
                    .gap_5()
                    .child(
                        h_flex()
                            .w_full()
                            .items_end()
                            .gap_3()
                            .flex_wrap()
                            .child(
                                div().flex_1().min_w(px(240.0)).child(self.page_header(
                                    t!("Playlists"),
                                    t!(
                                        "%{count} playlists",
                                        count = self.state.playlists.len()
                                    ),
                                    cx,
                                )),
                            )
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1()
                                    .child(
                                        Button::new("new-playlist")
                                            .label(t!("New playlist"))
                                            .primary()
                                            .small()
                                            .disabled(
                                                self.playlist_ui.busy || self.api.is_none(),
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.name_playlist(None, vec![], window, cx)
                                            })),
                                    )
                                    .child(
                                        Button::new("import-playlist")
                                            .label(t!("Import playlist"))
                                            .outline()
                                            .small()
                                            .disabled(self.api.is_none() || self.playlist_ui.busy)
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.open_import_dialog(window, cx);
                                            })),
                                    )
                                    .child(self.header_refresh_button(
                                        "refresh-playlists",
                                        t!("Refresh playlists"),
                                        self.playlist_ui.list_loading,
                                        cx,
                                        |this, _cx| this.load_playlists(),
                                    )),
                            ),
                    )
                    .when(
                        self.state.playlists.is_empty() && self.playlist_ui.list_loading,
                        |this| this.child(self.render_list_skeleton(8, cx)),
                    )
                    .when(
                        self.state.playlists.is_empty() && !self.playlist_ui.list_loading,
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
                                            .text_color(
                                                cx.theme().muted_foreground.opacity(0.72),
                                            ),
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
                    .when(!self.state.playlists.is_empty(), |this| {
                        this.child(playlist_list)
                    })
                    .into_any_element()
            }
            View::Search => {
                let mut content = v_flex()
                    .gap_5()
                    .child(self.page_header(
                        t!("Search"),
                        t!("Find music across the entire server library."),
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .child(Input::new(&self.search_input).w_full()),
                            )
                            .child(
                                Button::new("run-search")
                                    .label(t!("Search"))
                                    .primary()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.submit_search(cx);
                                        cx.notify();
                                    })),
                            )
                            .child(self.header_refresh_button(
                                "refresh-search",
                                t!("Run search again"),
                                false,
                                cx,
                                |this, cx| this.submit_search(cx),
                            )),
                    );
                if self.state.search_results.is_some() {
                    // 过滤只在有结果后出现；会话内状态，不持久化。
                    content =
                        content.child(h_flex().gap_2().children(SearchFilter::ALL.map(|filter| {
                            Button::new(SharedString::from(format!(
                                "search-filter-{}",
                                filter.label()
                            )))
                            .label(filter.label())
                            .ghost()
                            .small()
                            .selected(self.state.search_filter == filter)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.set_search_filter(filter, cx);
                                },
                            ))
                        })));
                }
                if let Some(results) = &self.state.search_results {
                    let filter = self.state.search_filter;
                    let show_artists =
                        filter == SearchFilter::All || filter == SearchFilter::Artists;
                    let show_albums = filter == SearchFilter::All || filter == SearchFilter::Albums;
                    let show_songs = filter == SearchFilter::All || filter == SearchFilter::Songs;
                    if show_artists {
                        if !results.artists.is_empty() {
                            content = content
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(t!("Artists")),
                                )
                                .child(self.render_artist_grid(&results.artists, cx, window));
                        } else if filter == SearchFilter::Artists {
                            content = content.child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("No artists matched your search.")),
                            );
                        }
                    }
                    if show_albums {
                        if !results.albums.is_empty() {
                            content = content
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(t!("Albums")),
                                )
                                .child(self.render_album_grid(
                                    "search",
                                    &results.albums,
                                    cx,
                                    window,
                                ));
                        } else if filter == SearchFilter::Albums {
                            content = content.child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("No albums matched your search.")),
                            );
                        }
                    }
                    if show_songs {
                        if !results.songs.is_empty() {
                            content = content
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(t!("Songs")),
                                )
                                .child(self.render_song_list("search", false, cx));
                        } else if filter == SearchFilter::Songs {
                            content = content.child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("No songs matched your search.")),
                            );
                        }
                    }
                    if filter == SearchFilter::All
                        && results.artists.is_empty()
                        && results.albums.is_empty()
                        && results.songs.is_empty()
                    {
                        content = content.child(
                            div()
                                .py_8()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("No results matched your search.")),
                        );
                    }
                }
                content.into_any_element()
            }
            View::ArtistDetail => self.render_artist_detail(window, cx),
            View::AlbumDetail => self.render_album_detail(cx),
            View::PlaylistDetail => self.render_playlist_detail(cx),
            View::NowPlaying => self.render_now_playing(window, cx),
        }
    }
}
