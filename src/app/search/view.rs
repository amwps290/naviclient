//! 全库搜索输入、分类筛选及结果渲染。

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn render_search(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
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
                        self.state.search.loading(),
                        cx,
                        |this, cx| this.submit_search(cx),
                    )),
            );
        if self.state.search.loading() && self.state.search.results().is_none() {
            content = content.child(self.render_list_skeleton(6, cx));
        }
        if self.state.search.results().is_some() {
            // 过滤只在有结果后出现；会话内状态，不持久化。
            content = content.child(h_flex().gap_2().children(SearchFilter::ALL.map(|filter| {
                Button::new(SharedString::from(format!(
                    "search-filter-{}",
                    filter.label()
                )))
                .label(filter.label())
                .ghost()
                .small()
                .selected(self.state.search.filter() == filter)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.set_search_filter(filter, cx);
                }))
            })));
        }
        if let Some(results) = self.state.search.results() {
            let filter = self.state.search.filter();
            let show_artists = filter == SearchFilter::All || filter == SearchFilter::Artists;
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
                        .child(self.render_album_grid("search", &results.albums, cx, window));
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
                        .child(self.render_song_list(SongScope::Search, cx));
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
}
