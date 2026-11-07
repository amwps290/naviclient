//! 全库搜索输入、分类筛选及结果渲染。

use crate::app::prelude::*;
use crate::app::widgets::shelf::shelf_section;

impl NavidromeApp {
    pub(in crate::app) fn render_search(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let mut content = v_flex().gap(px(tokens::SECTION_GAP)).child(
            v_flex()
                .gap_6()
                .child(self.page_header(
                    t!("Search"),
                    t!("Find music across the entire server library."),
                    cx,
                ))
                .child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .items_center()
                        .child(
                            h_flex()
                                .flex_1()
                                .min_w_0()
                                .h(px(tokens::SEARCH_CONTROL_HEIGHT))
                                .px_4()
                                .items_center()
                                .rounded(px(tokens::radius::FULL))
                                .bg(p.sunken)
                                .child(
                                    Input::new(&self.search_input)
                                        .appearance(false)
                                        .w_full()
                                        .min_w_0()
                                        .px_0()
                                        .gap_3()
                                        .text_size(px(tokens::BODY))
                                        .prefix(
                                            Icon::new(KitIcon::Search)
                                                .small()
                                                .text_color(p.text_muted),
                                        ),
                                ),
                        )
                        .child(
                            Button::new("run-search")
                                .label(t!("Search"))
                                .primary()
                                .h(px(tokens::SEARCH_CONTROL_HEIGHT))
                                .px_6()
                                .rounded(px(tokens::radius::FULL))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.submit_search(cx);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("refresh-search")
                                .icon(AppIcon::Refresh)
                                .accessibility_label(t!("Run search again"))
                                .tooltip(t!("Run search again"))
                                .ghost()
                                .small()
                                .w(px(tokens::SEARCH_CONTROL_HEIGHT))
                                .h(px(tokens::SEARCH_CONTROL_HEIGHT))
                                .rounded(px(tokens::radius::FULL))
                                .loading(self.state.search.loading())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.submit_search(cx);
                                    cx.notify();
                                })),
                        ),
                ),
        );
        if self.state.search.loading() && self.state.search.results().is_none() {
            content = content.child(self.render_list_skeleton(6, cx));
        }
        if self.state.search.results().is_some() {
            // 过滤只在有结果后出现；会话内状态，不持久化。
            content = content.child(h_flex().flex_wrap().items_center().gap_2().children(
                SearchFilter::ALL.map(|filter| {
                    let selected = self.state.search.filter() == filter;
                    Button::new(SharedString::from(format!(
                        "search-filter-{}",
                        filter.label()
                    )))
                    .label(filter.label())
                    .custom(soft_toggle_style(selected, cx))
                    .small()
                    .px_4()
                    .rounded(px(tokens::radius::FULL))
                    .selected(selected)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_search_filter(filter, cx);
                    }))
                }),
            ));
        }
        if let Some(results) = self.state.search.results() {
            let filter = self.state.search.filter();
            let show_artists = filter == SearchFilter::All || filter == SearchFilter::Artists;
            let show_albums = filter == SearchFilter::All || filter == SearchFilter::Albums;
            let show_songs = filter == SearchFilter::All || filter == SearchFilter::Songs;
            if show_artists {
                if !results.artists.is_empty() {
                    content = content.child(shelf_section(
                        section_title(t!("Artists"), cx).into_any_element(),
                        self.render_artist_grid(&results.artists, cx, window),
                    ));
                } else if filter == SearchFilter::Artists {
                    content = content.child(
                        v_flex()
                            .py_12()
                            .items_center()
                            .gap_3()
                            .child(
                                Icon::new(AppIcon::Artists)
                                    .with_size(px(48.0))
                                    .text_color(cx.theme().muted_foreground.opacity(0.4)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("No artists matched your search.")),
                            ),
                    );
                }
            }
            if show_albums {
                if !results.albums.is_empty() {
                    content = content.child(shelf_section(
                        section_title(t!("Albums"), cx).into_any_element(),
                        self.render_album_grid("search", &results.albums, cx, window),
                    ));
                } else if filter == SearchFilter::Albums {
                    content = content.child(
                        v_flex()
                            .py_12()
                            .items_center()
                            .gap_3()
                            .child(
                                Icon::new(AppIcon::Albums)
                                    .with_size(px(48.0))
                                    .text_color(cx.theme().muted_foreground.opacity(0.4)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("No albums matched your search.")),
                            ),
                    );
                }
            }
            if show_songs {
                if !results.songs.is_empty() {
                    content = content.child(shelf_section(
                        section_title(t!("Songs"), cx).into_any_element(),
                        self.render_song_list(SongScope::Search, cx),
                    ));
                } else if filter == SearchFilter::Songs {
                    content = content.child(
                        v_flex()
                            .py_12()
                            .items_center()
                            .gap_3()
                            .child(
                                Icon::new(AppIcon::Playlists)
                                    .with_size(px(48.0))
                                    .text_color(cx.theme().muted_foreground.opacity(0.4)),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(t!("No songs matched your search.")),
                            ),
                    );
                }
            }
            if filter == SearchFilter::All
                && results.artists.is_empty()
                && results.albums.is_empty()
                && results.songs.is_empty()
            {
                content = content.child(
                    v_flex()
                        .py_12()
                        .items_center()
                        .gap_3()
                        .child(
                            Icon::new(AppIcon::Albums)
                                .with_size(px(56.0))
                                .text_color(cx.theme().muted_foreground.opacity(0.4)),
                        )
                        .child(
                            div()
                                .text_base()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(cx.theme().foreground)
                                .child(t!("No results matched your search.")),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(t!("Try adjusting your search terms or check spelling.")),
                        ),
                );
            }
        }
        content.into_any_element()
    }
}
