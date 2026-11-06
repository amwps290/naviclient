//! 收藏、艺术家、专辑和流派集合页渲染。

use gpui_kit::component::menu::PopupMenuItem;

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn render_library_browse(
        &self,
        view: View,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        match view {
            View::Favorites => {
                let favorites = self.state.favorites.items();
                let total = self.state.favorites.total();
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
                            self.state.favorites.loading(),
                            cx,
                            |this, _cx| this.load_favorites(),
                        )),
                );

                if total == 0 && self.state.favorites.loading() {
                    content = content.child(self.render_list_skeleton(6, cx));
                } else if total == 0 {
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
                        .child(self.render_song_list(SongScope::Favorites, cx));
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
                            self.state.artists.loading() && self.state.artists.is_empty(),
                            cx,
                            |this, _cx| this.load_artists(),
                        )),
                )
                .child(
                    if self.state.artists.is_empty() && self.state.artists.loading() {
                        self.render_artist_skeleton_grid(cx)
                    } else {
                        self.render_artist_grid(self.state.artists.visible_items(), cx, window)
                    },
                )
                .into_any_element(),
            View::Albums => {
                v_flex()
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
                                self.state.albums.loading(),
                                cx,
                                |this, _cx| this.load_albums(),
                            )),
                    )
                    .child(
                        if self.state.albums.is_empty() && self.state.albums.loading() {
                            self.render_album_skeleton_grid(cx)
                        } else {
                            self.render_album_grid("albums", self.state.albums.items(), cx, window)
                        },
                    )
                    .when(self.state.albums.failed(), |this| {
                        this.child(self.render_retry_row("retry-albums", cx, |this, _cx| {
                            this.load_more_albums()
                        }))
                    })
                    .into_any_element()
            }
            View::Genres => {
                // 有旧数据（磁盘缓存）时直接渲染，仅在后台重验证中提示；
                // 首次无缓存才显示骨架屏。
                let subtitle = if self.state.genres.loading() && !self.state.genres.is_empty() {
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
                                self.state.genres.loading(),
                                cx,
                                |this, _cx| this.load_genres(),
                            )),
                    )
                    .child(
                        if self.state.genres.is_empty() && self.state.genres.loading() {
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
                let Some(detail) = self.state.genres.detail() else {
                    return div().child(t!("Genre not found")).into_any_element();
                };
                v_flex()
                    .gap_5()
                    .child(self.page_header(
                        detail.name().to_string(),
                        t!("%{count} albums", count = detail.albums().len()),
                        cx,
                    ))
                    .child(if detail.albums().is_empty() && detail.albums().loading() {
                        self.render_album_skeleton_grid(cx)
                    } else if detail.albums().is_empty() {
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!("No albums in this genre yet."))
                            .into_any_element()
                    } else {
                        self.render_album_grid("genre-detail", detail.albums().items(), cx, window)
                    })
                    .when(detail.albums().failed(), |this| {
                        this.child(
                            self.render_retry_row("retry-genre-albums", cx, |this, _cx| {
                                this.load_more_genre_albums()
                            }),
                        )
                    })
                    .into_any_element()
            }
            _ => unreachable!("non-library view passed to render_library_browse"),
        }
    }

    /// 专辑页排序菜单：当前方式打勾；切换后持久化并从第一页重新加载。
    fn render_album_sort_button(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let app = cx.entity().downgrade();
        Button::new("albums-sort")
            .label(t!("Sort: %{mode}", mode = self.state.albums_sort.label()))
            .icon(AppIcon::ChevronDown)
            .tooltip(t!("Album sort order"))
            .ghost()
            .small()
            .dropdown_menu(move |menu, _, cx| {
                app.update(cx, |this, cx| {
                    let mut menu = menu;
                    for sort in AlbumSort::ALL {
                        let checked = this.state.albums_sort == sort;
                        menu =
                            menu.item(PopupMenuItem::new(sort.label()).checked(checked).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.set_album_sort(sort, cx);
                                }),
                            ));
                    }
                    menu
                })
                .expect("app owns album sort menu")
            })
            .into_any_element()
    }

    /// 艺术家页排序菜单：全量列表在客户端重排，切换立即生效。
    fn render_artist_sort_button(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let app = cx.entity().downgrade();
        Button::new("artists-sort")
            .label(t!(
                "Sort: %{mode}",
                mode = self.state.artists.sort().label()
            ))
            .icon(AppIcon::ChevronDown)
            .tooltip(t!("Artist sort order"))
            .ghost()
            .small()
            .dropdown_menu(move |menu, _, cx| {
                app.update(cx, |this, cx| {
                    let mut menu = menu;
                    for sort in ArtistSort::ALL {
                        let checked = this.state.artists.sort() == sort;
                        menu =
                            menu.item(PopupMenuItem::new(sort.label()).checked(checked).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.set_artist_sort(sort, cx);
                                }),
                            ));
                    }
                    menu
                })
                .expect("app owns artist sort menu")
            })
            .into_any_element()
    }

    /// 流派卡片网格：卡片显示流派名与专辑/歌曲数，点击进入流派详情。
    fn render_genre_grid(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let genres = self.state.genres.items();
        h_flex()
            .items_start()
            .flex_wrap()
            .gap_4()
            .children(genres.iter().enumerate().map(|(index, genre)| {
                let genre_for_click = genre.value.clone();
                let counts = match (genre.album_count, genre.song_count) {
                    (Some(albums), Some(songs)) => t!(
                        "%{albums} albums · %{songs} songs",
                        albums = albums,
                        songs = songs
                    ),
                    (Some(albums), None) => t!("%{albums} albums", albums = albums),
                    (None, Some(songs)) => t!("%{songs} songs", songs = songs),
                    (None, None) => t!("—"),
                };
                div()
                    .id(SharedString::from(format!("genre-card-{index}")))
                    .w(px(232.0))
                    .p_4()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border)
                    .cursor_pointer()
                    .hover(|style| style.bg(cx.theme().accent.opacity(0.1)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_genre(genre_for_click.clone());
                        cx.notify();
                    }))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(genre.value.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(counts),
                    )
            }))
            .into_any_element()
    }
}
