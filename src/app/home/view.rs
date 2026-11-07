//! Home page composition: a greeting above a stack of shelves.

use super::greeting::current_greeting;
use crate::app::prelude::*;
use crate::app::widgets::shelf::hero_shelf;

impl NavidromeApp {
    pub(in crate::app) fn render_home(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let columns = self.content_columns(window).count;
        let compact = self.shell_layout(window).compact;
        let recent = self.state.home_albums.albums(HomeAlbumSection::Recent);
        let frequent = self.state.home_albums.albums(HomeAlbumSection::Frequent);
        let newest = self.state.home_albums.albums(HomeAlbumSection::Newest);
        let random = self.state.home_albums.albums(HomeAlbumSection::Random);
        let feature_uses_recent = self.state.now_playing.is_none() && !recent.is_empty();
        let recent_start = usize::from(feature_uses_recent);
        let recent_end = (recent_start + columns).min(recent.len());
        let recent_preview = &recent[recent_start..recent_end];
        let frequent_preview = &frequent[..frequent.len().min(columns)];
        let newest_preview = &newest[..newest.len().min(columns)];
        let random_preview = &random[..random.len().min(columns)];
        let mut cards = Vec::with_capacity(2);
        cards.extend(self.render_home_feature(compact, cx));
        cards.push(self.render_daily_mix_card(compact, cx));

        v_flex()
            .gap(px(tokens::SECTION_GAP))
            .child(
                v_flex()
                    .gap_6()
                    .child(serif_heading(current_greeting(), tokens::PAGE_TITLE, cx))
                    .child(hero_shelf(cards)),
            )
            .when(
                !recent_preview.is_empty()
                    || self.state.home_albums.loading(HomeAlbumSection::Recent),
                |this| {
                    this.child(self.render_home_primary_section(
                        t!("Recently played"),
                        self.render_home_album_content(
                            "recent",
                            recent_preview,
                            self.state.home_albums.loading(HomeAlbumSection::Recent),
                            t!("Nothing played yet — recently played albums will appear here."),
                            cx,
                            window,
                        ),
                        cx,
                    ))
                },
            )
            .child(self.render_home_section(
                "frequent",
                t!("Frequently played"),
                || {
                    self.render_home_album_content(
                    "frequent",
                    frequent_preview,
                    self.state.home_albums.loading(HomeAlbumSection::Frequent),
                    t!("Nothing frequently played yet — your most-played albums will appear here."),
                    cx,
                    window,
                )
                },
                cx,
            ))
            .child(self.render_home_section(
                "newest",
                t!("Newest albums"),
                || {
                    self.render_home_album_content(
                        "newest",
                        newest_preview,
                        self.state.home_albums.loading(HomeAlbumSection::Newest),
                        t!("No albums in your library yet."),
                        cx,
                        window,
                    )
                },
                cx,
            ))
            .child(self.render_home_section(
                "random",
                t!("Random albums"),
                || {
                    self.render_home_album_content(
                        "random",
                        random_preview,
                        self.state.home_albums.loading(HomeAlbumSection::Random),
                        t!("No random albums to show — refresh the library for a fresh pick."),
                        cx,
                        window,
                    )
                },
                cx,
            ))
            .into_any_element()
    }
}
