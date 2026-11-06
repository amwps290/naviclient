//! Home page composition and bounded album previews.

use super::*;

/// Six 176px album cards with five 20px gaps: Home previews intentionally
/// stop at six items, so the listening grid should share the same visual edge
/// instead of stretching indefinitely on ultrawide/fullscreen windows.
const HOME_PREVIEW_MAX_WIDTH: f32 = 6.0 * 176.0 + 5.0 * 20.0;

impl NavidromeApp {
    pub(super) fn render_home(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let recent = self.state.home_albums.albums(HomeAlbumSection::Recent);
        let frequent = self.state.home_albums.albums(HomeAlbumSection::Frequent);
        let newest = self.state.home_albums.albums(HomeAlbumSection::Newest);
        let random = self.state.home_albums.albums(HomeAlbumSection::Random);
        let feature_uses_recent = self.state.now_playing.is_none() && !recent.is_empty();
        let recent_start = usize::from(feature_uses_recent);
        let recent_end = (recent_start + 6).min(recent.len());
        let recent_preview = &recent[recent_start..recent_end];
        let frequent_preview = &frequent[..frequent.len().min(6)];
        let newest_preview = &newest[..newest.len().min(6)];
        let random_preview = &random[..random.len().min(6)];
        let status = if self.state.connection.server().is_some() {
            format!("{} · {}", self.active_server_label(), t!("Connected"))
        } else {
            self.state.status.clone()
        };
        let listening_feature = self.render_home_feature(window, cx);
        let daily_mix_card = self.render_daily_mix_card(window, cx);

        v_flex()
            .gap_6()
            .child(self.page_header(t!("Home"), status, cx))
            .child(
                h_flex()
                    .w_full()
                    .max_w(px(HOME_PREVIEW_MAX_WIDTH))
                    .gap_4()
                    .items_stretch()
                    .when_some(listening_feature, |this, feature| {
                        this.child(div().flex_1().min_w_0().child(feature))
                    })
                    .child(div().flex_1().min_w_0().child(daily_mix_card)),
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
