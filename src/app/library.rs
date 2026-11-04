//! 媒体库数据加载请求、导航入口与收藏切换。

use super::*;

impl NavidromeApp {
    pub(super) fn refresh_library(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.state.loading = true;
        self.state.status = t!("Connecting to server...").to_string();
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::Ping(api.ping().await.map_err(error_message)));
        });
        self.load_artists();
        self.load_albums();
        self.load_genres();
        self.load_playlists();
        self.load_favorites();
        self.load_newest_albums();
        self.load_recent_albums();
        self.load_frequent_albums();
        self.load_random_albums();
    }

    pub(super) fn load_open_subsonic_info(&mut self) {
        let Some(api) = self.api.clone() else { return };
        let base_url = api.base_url().to_string();
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.open_subsonic_info().await.map_err(error_message);
            let _ = tx.send(Msg::OpenSubsonicInfo { base_url, result });
        });
    }

    /// 加载全部流派（`getGenres`）。
    pub(super) fn load_genres(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.state.genres_loading = true;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::Genres(api.genres().await.map_err(error_message)));
        });
    }

    /// 打开流派详情页并从第一页加载该流派的专辑。
    pub(super) fn open_genre(&mut self, genre: String) {
        self.state.current_genre = Some(genre.clone());
        self.state.genre_albums.clear();
        self.state.genre_albums_page = 0;
        self.state.genre_albums_exhausted = false;
        self.state.genre_albums_loading = true;
        self.state.view = View::GenreDetail;
        let Some(api) = self.api.clone() else { return };
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api
                .albums_by_genre(&genre, ALBUM_PAGE_SIZE as u32, 0)
                .await
                .map_err(error_message);
            let _ = tx.send(Msg::GenreAlbums { genre, result });
        });
    }

    /// 流派详情页接近底部时追加下一页。
    pub(super) fn load_more_genre_albums(&mut self) {
        if self.state.genre_albums_loading || self.state.genre_albums_exhausted {
            return;
        }
        let Some(api) = self.api.clone() else { return };
        let Some(genre) = self.state.current_genre.clone() else {
            return;
        };
        self.state.genre_albums_loading = true;
        self.state.genre_albums_page_failed = false;
        let offset = album_page_offset(self.state.genre_albums_page, ALBUM_PAGE_SIZE);
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api
                .albums_by_genre(&genre, ALBUM_PAGE_SIZE as u32, offset)
                .await
                .map_err(error_message);
            let _ = tx.send(Msg::GenreAlbums { genre, result });
        });
    }

    /// 切换搜索结果显示过滤（会话内状态，不持久化）。
    pub(super) fn set_search_filter(&mut self, filter: SearchFilter, cx: &mut Context<Self>) {
        if self.state.search_filter == filter {
            return;
        }
        self.state.search_filter = filter;
        cx.notify();
    }

    pub(super) fn load_artists(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.state.loading = true;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::Artists(api.artists().await.map_err(error_message)));
        });
    }

    pub(super) fn load_albums(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.state.loading = true;
        self.state.albums_loading = true;
        self.state.albums_exhausted = false;
        self.state.albums_page = 0;
        self.state.albums.clear();
        let sort = self.state.albums_sort;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::Albums(
                api.albums(ALBUM_PAGE_SIZE as u32, 0, sort)
                    .await
                    .map_err(error_message),
            ));
        });
    }

    /// 首页“最近添加”区块（与专辑页排序状态解耦）。
    pub(super) fn load_newest_albums(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.state.newest_albums_loading = true;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::NewestAlbums(
                api.albums_newest(30).await.map_err(error_message),
            ));
        });
    }

    /// 加载最近播放的专辑（Home 页展示）。
    pub(super) fn load_recent_albums(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.state.recent_albums_loading = true;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::RecentAlbums(
                api.albums_recent(30).await.map_err(error_message),
            ));
        });
    }

    /// 加载播放次数最多的专辑（Home 页展示）。
    pub(super) fn load_frequent_albums(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.state.frequent_albums_loading = true;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::FrequentAlbums(
                api.albums_frequent(30).await.map_err(error_message),
            ));
        });
    }

    /// 加载随机专辑（Home 页展示）。
    pub(super) fn load_random_albums(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.state.random_albums_loading = true;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::RandomAlbums(
                api.albums_random(30).await.map_err(error_message),
            ));
        });
    }

    pub(super) fn load_more_albums(&mut self) {
        let Some(api) = self.api.clone() else { return };
        if self.state.albums_loading || self.state.albums_exhausted {
            return;
        }
        self.state.albums_loading = true;
        let offset = album_page_offset(self.state.albums_page, ALBUM_PAGE_SIZE);
        let sort = self.state.albums_sort;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::Albums(
                api.albums(ALBUM_PAGE_SIZE as u32, offset, sort)
                    .await
                    .map_err(error_message),
            ));
        });
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

    /// 切换艺术家排序：全量列表已在客户端，原地重排并重置增量渲染进度。
    pub(super) fn set_artist_sort(&mut self, sort: ArtistSort, cx: &mut Context<Self>) {
        if self.state.artists_sort == sort {
            return;
        }
        self.state.artists_sort = sort;
        self.config.artists_sort = sort;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Artist sort save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        sort_artists(&mut self.state.artists, sort);
        self.state.artists_visible = self.state.artists.len().min(300);
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
            View::Artists if self.state.artists_visible < self.state.artists.len() => {
                self.state.artists_visible =
                    (self.state.artists_visible + 300).min(self.state.artists.len());
            }
            View::Playlists if self.state.playlists_visible < self.state.playlists.len() => {
                self.state.playlists_visible =
                    (self.state.playlists_visible + 100).min(self.state.playlists.len());
            }
            _ => {}
        }
    }

    pub(super) fn load_playlists(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.playlist_ui.list_loading = true;
        self.playlist_ui.list_request = self.playlist_ui.list_request.wrapping_add(1);
        let request = self.playlist_ui.list_request;
        let generation = self.playlist_ui.generation;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.playlists().await.map_err(error_message);
            let _ = tx.send(Msg::Playlists {
                generation,
                request,
                result,
            });
        });
    }

    pub(super) fn load_favorites(&mut self) {
        let Some(api) = self.api.clone() else { return };
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let _ = tx.send(Msg::Favorites(api.favorites().await.map_err(error_message)));
        });
    }

    pub(super) fn open_artist(&mut self, artist: Artist) {
        self.clear_song_multi();
        self.state.current_artist = Some(artist.clone());
        self.state.artist_albums.clear();
        self.state.artist_top_songs.clear();
        self.state.artist_top_songs_error = None;
        self.state.view = View::ArtistDetail;
        self.ensure_cover(artist.cover_art.as_deref(), true);
        self.load_artist_top_songs();
        let Some(api) = self.api.clone() else { return };
        let artist_id = artist.id.clone();
        let tx = self.tx.clone();
        self.state.loading = true;
        self.spawn_future(async move {
            let result = api.artist_albums(&artist_id).await.map_err(error_message);
            let _ = tx.send(Msg::ArtistAlbums { artist_id, result });
        });
    }

    /// Load a compact popular-song section for the active artist. Servers that
    /// advertise `topSongsByArtistId` use the stable id; compatibility-mode
    /// servers retain the standard display-name request.
    pub(super) fn load_artist_top_songs(&mut self) {
        let Some(artist) = self.state.current_artist.clone() else {
            return;
        };
        let Some(api) = self.api.clone() else { return };
        let artist_id_support = self
            .state
            .server
            .as_ref()
            .map(|server| server.capabilities.top_songs_by_artist_id)
            .unwrap_or_default();
        self.state.artist_top_songs_loading = true;
        self.state.artist_top_songs_error = None;
        self.artist_top_songs_generation = self.artist_top_songs_generation.wrapping_add(1);
        let generation = self.artist_top_songs_generation;
        let artist_id = artist.id.clone();
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api
                .artist_top_songs(&artist, 12, artist_id_support)
                .await
                .map_err(error_message);
            let _ = tx.send(Msg::ArtistTopSongs {
                generation,
                artist_id,
                result,
            });
        });
    }

    pub(super) fn play_artist_top_songs(&mut self) {
        let songs = self.state.artist_top_songs.clone();
        if !songs.is_empty() {
            self.play_song_list(&songs, 0);
        }
    }

    pub(super) fn start_artist_radio(&mut self) {
        if let (Some(artist), Some(initial_song)) = (
            self.state.current_artist.clone(),
            self.state.artist_top_songs.first().cloned(),
        ) {
            self.start_artist_radio_from_seed(artist, initial_song);
        }
    }

    /// 从歌曲元数据打开艺术家页；缺少 artist_id 时无操作。
    pub(super) fn open_song_artist(&mut self, song: &Song) {
        let Some(artist_id) = song.artist_id.clone() else {
            return;
        };
        self.open_artist(Artist {
            id: artist_id,
            name: song.artist.clone(),
            album_count: None,
            cover_art: song.cover_art.clone(),
        });
    }

    /// 从歌曲元数据打开专辑页；缺少 album_id 时无操作。
    pub(super) fn open_song_album(&mut self, song: &Song) {
        let Some(album_id) = song.album_id.clone() else {
            return;
        };
        let name = if song.album.trim().is_empty() {
            t!("Unknown album").to_string()
        } else {
            song.album.clone()
        };
        self.open_album(Album {
            id: album_id,
            name,
            artist: song.artist.clone(),
            cover_art: song.cover_art.clone(),
            ..Album::default()
        });
    }

    pub(super) fn open_album(&mut self, album: Album) {
        self.clear_song_multi();
        self.state.current_album = Some(album.clone());
        self.state.current_songs.clear();
        self.state.view = View::AlbumDetail;
        self.ensure_cover(album.cover_art.as_deref(), true);
        let Some(api) = self.api.clone() else { return };
        let album_id = album.id.clone();
        let tx = self.tx.clone();
        self.state.loading = true;
        self.spawn_future(async move {
            let result = api.album_songs(&album_id).await.map_err(error_message);
            let _ = tx.send(Msg::AlbumSongs { album_id, result });
        });
    }

    /// 立即播放整张专辑：加载歌曲后在后台开始播放（不切换视图）。
    pub(super) fn play_album(&mut self, album: Album) {
        self.state.current_album = Some(album.clone());
        self.state.pending_play_album = true;
        self.ensure_cover(album.cover_art.as_deref(), true);
        let Some(api) = self.api.clone() else { return };
        let album_id = album.id.clone();
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.album_songs(&album_id).await.map_err(error_message);
            let _ = tx.send(Msg::AlbumSongs { album_id, result });
        });
    }

    /// 立即播放艺术家的全部歌曲（聚合其所有专辑的曲目）。
    pub(super) fn play_artist(&mut self, artist: Artist) {
        self.state.current_artist = Some(artist.clone());
        self.ensure_cover(artist.cover_art.as_deref(), true);
        let Some(api) = self.api.clone() else { return };
        let artist_id = artist.id.clone();
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = async {
                let albums = api.artist_albums(&artist_id).await?;
                let mut songs = Vec::new();
                for album in albums {
                    songs.extend(api.album_songs(&album.id).await?);
                }
                Ok::<_, anyhow::Error>(songs)
            }
            .await
            .map_err(error_message);
            let _ = tx.send(Msg::PlayArtistSongs(result));
        });
    }

    pub(super) fn open_playlist(&mut self, playlist: Playlist) {
        self.state.current_playlist = Some(playlist.clone());
        self.state.playlist_songs.clear();
        self.state.view = View::PlaylistDetail;
        self.ensure_cover(playlist.cover_art.as_deref(), true);
        self.load_playlist_detail(playlist.id);
    }

    pub(super) fn submit_search(&mut self, cx: &Context<Self>) {
        self.clear_song_multi();
        let query = self.search_input.read(cx).value().trim().to_string();
        if query.is_empty() {
            self.state.search_results = None;
            return;
        }
        let Some(api) = self.api.clone() else { return };
        self.state.loading = true;
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.search(&query, 120, 80, 60).await.map_err(error_message);
            let _ = tx.send(Msg::Search(result));
        });
    }

    pub(super) fn toggle_favorite(&mut self, key: FavoriteKey) {
        let Some(api) = self.api.clone() else { return };
        if !self.state.pending_favorites.insert(key.clone()) {
            return;
        }

        let starred = !self.state.favorite_ids.contains(&key);
        if starred {
            self.state.favorite_ids.insert(key.clone());
        } else {
            self.state.favorite_ids.remove(&key);
        }

        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.set_favorite(&key, starred).await.map_err(error_message);
            let _ = tx.send(Msg::FavoriteChanged {
                key,
                starred,
                result,
            });
        });
    }
}
