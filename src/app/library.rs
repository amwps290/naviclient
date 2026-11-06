//! 媒体库刷新、导航与列表排序。

use super::library_navigation::{album_for_song, artist_for_song};
use super::*;

impl NavidromeApp {
    pub(super) fn refresh_library(&mut self) {
        if self.api.is_none() {
            return;
        }
        self.refresh_connection();
        self.load_artists();
        self.load_albums();
        self.load_genres();
        self.load_playlists();
        self.load_favorites();
        for section in HomeAlbumSection::ALL {
            self.load_home_albums(section);
        }
    }

    /// 切换专辑排序：更新状态并持久化，然后重新从第一页加载。
    pub(super) fn set_album_sort(&mut self, sort: AlbumSort, cx: &mut Context<Self>) {
        if self.state.albums_sort == sort {
            return;
        }
        self.state.albums_sort = sort;
        self.config.albums_sort = sort;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Album sort save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        self.load_albums();
        cx.notify();
    }

    pub(super) fn maybe_load_more_content(&mut self) {
        // 长列表增量渲染 / 分页：接近底部时追加。
        // 歌曲列表已改为 uniform_list 视口虚拟化，不再需要逐批追加行。
        let max = self.content_scroll_handle.max_offset().y;
        // GPUI stores the translated content position as a negative offset.
        let offset = -self.content_scroll_handle.offset().y;
        if max <= px(0.0) || f32::from(max - offset) >= 600.0 {
            return;
        }
        match self.state.view {
            View::Albums => self.load_more_albums(),
            View::GenreDetail => self.load_more_genre_albums(),
            View::Artists => self.state.artists.show_more(),
            View::Playlists => self.state.playlists.show_more(),
            _ => {}
        }
    }

    /// 从歌曲元数据打开艺术家页；缺少 artist_id 时无操作。
    pub(super) fn open_song_artist(&mut self, song: &Song) {
        if let Some(artist) = artist_for_song(song) {
            self.open_artist(artist);
        }
    }

    /// 从歌曲元数据打开专辑页；缺少 album_id 时无操作。
    pub(super) fn open_song_album(&mut self, song: &Song) {
        if let Some(album) = album_for_song(song) {
            self.open_album(album);
        }
    }
}
