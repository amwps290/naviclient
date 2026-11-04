//! 主内容区视图路由。

use super::*;

impl NavidromeApp {
    pub(super) fn render_content(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        match self.state.view {
            View::Home => self.render_home(window, cx),
            view @ (View::Favorites
            | View::Artists
            | View::Albums
            | View::Genres
            | View::GenreDetail) => self.render_library_browse(view, window, cx),
            View::Playlists => self.render_playlists(cx),
            View::Search => self.render_search(window, cx),
            View::ArtistDetail => self.render_artist_detail(window, cx),
            View::AlbumDetail => self.render_album_detail(cx),
            View::PlaylistDetail => self.render_playlist_detail(cx),
            View::NowPlaying => self.render_now_playing(window, cx),
        }
    }
}
