//! 主内容区视图路由。

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn render_content(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        match self.state.view {
            View::Home if self.browsing_bilibili() => self.render_bilibili_home(window, cx),
            View::Home => self.render_home(window, cx),
            view @ (View::Favorites
            | View::Artists
            | View::Albums
            | View::Genres
            | View::GenreDetail) => self.render_library_browse(view, window, cx),
            View::Folders => self.render_folders_view(window, cx),
            View::Playlists if self.browsing_bilibili() => self.render_bilibili_folders(window, cx),
            View::Playlists => self.render_playlists(window, cx),
            View::Search if self.browsing_bilibili() => self.render_bilibili_discovery(window, cx),
            View::Search => self.render_search(window, cx),
            View::ArtistDetail => self.render_artist_detail(window, cx),
            View::AlbumDetail => self.render_album_detail(window, cx),
            View::PlaylistDetail => self.render_playlist_detail(window, cx),
            View::LocalPlaylistDetail => self.render_local_playlist_detail(window, cx),
            View::NowPlaying => self.render_now_playing(window, cx),
        }
    }
}
