//! 媒体库数据：各视图的加载请求、导航入口、收藏切换与
//! 后台消息/托盘命令轮询。

use super::*;

impl NavidromeApp {
    pub(super) fn refresh_library(&mut self) {
        let Some(api) = self.api.clone() else { return };
        self.state.loading = true;
        self.state.status = "Connecting to server...".to_string();
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
            self.notify_error(format!("Album sort save failed: {error:#}"));
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
            self.notify_error(format!("Artist sort save failed: {error:#}"));
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
        self.state.current_artist = Some(artist.clone());
        self.state.artist_albums.clear();
        self.state.view = View::ArtistDetail;
        self.ensure_cover(artist.cover_art.as_deref(), true);
        let Some(api) = self.api.clone() else { return };
        let artist_id = artist.id.clone();
        let tx = self.tx.clone();
        self.state.loading = true;
        self.spawn_future(async move {
            let result = api.artist_albums(&artist_id).await.map_err(error_message);
            let _ = tx.send(Msg::ArtistAlbums { artist_id, result });
        });
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
            "Unknown album".to_string()
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

    pub(super) fn poll_tray_commands(&mut self) -> bool {
        let mut changed = false;
        while let Ok(command) = self.tray_rx.try_recv() {
            changed = true;
            match command {
                TrayCommand::TogglePlayback => self.toggle_playback(),
                TrayCommand::Previous => self.skip(-1),
                TrayCommand::Next => self.skip(1),
                TrayCommand::ShowWindow => {
                    if let Some(hwnd) = self.main_hwnd {
                        show_main_window(hwnd);
                    }
                }
                TrayCommand::Quit => {
                    self.quitting.store(true, Ordering::Relaxed);
                    if let Some(hwnd) = self.main_hwnd {
                        request_window_close(hwnd);
                    }
                }
            }
        }
        changed
    }

    pub(super) fn poll_messages(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut changed = false;
        while let Ok(message) = self.rx.try_recv() {
            changed = true;
            match message {
                Msg::Ping(result) => {
                    self.state.loading = false;
                    match result {
                        Ok(info) => {
                            let server_name = info.app_name.as_deref().unwrap_or("Navidrome");
                            self.state.status = format!(
                                "{server_name} {} - {}",
                                info.version,
                                self.api
                                    .as_ref()
                                    .map(|api| api.base_url().to_string())
                                    .unwrap_or_default()
                            );
                            self.state.server = Some(info);
                        }
                        Err(error) => {
                            self.state.status = "Connection failed".to_string();
                            self.notify_error(format!("Connection failed: {error}"));
                        }
                    }
                }
                Msg::Artists(result) => {
                    self.state.loading = false;
                    match result {
                        Ok(mut artists) => {
                            sort_artists(&mut artists, self.state.artists_sort);
                            let covers = artists
                                .iter()
                                .filter_map(|item| item.cover_art.clone())
                                .take(40)
                                .collect();
                            self.state.artists_visible = artists.len().min(300);
                            self.state.artists = artists;
                            self.preload_covers(covers);
                        }
                        Err(error) => self.notify_error(error),
                    }
                }
                Msg::ArtistAlbums { artist_id, result } => {
                    self.state.loading = false;
                    if self.state.current_artist.as_ref().map(|a| a.id.as_str())
                        == Some(artist_id.as_str())
                    {
                        match result {
                            Ok(albums) => {
                                let covers = albums
                                    .iter()
                                    .filter_map(|item| item.cover_art.clone())
                                    .collect();
                                self.state.artist_albums = albums;
                                self.preload_covers(covers);
                            }
                            Err(error) => self.notify_error(error),
                        }
                    }
                }
                Msg::Albums(result) => {
                    self.state.loading = false;
                    self.state.albums_loading = false;
                    let handle_start = std::time::Instant::now();
                    match result {
                        Ok(mut albums) => {
                            if albums.is_empty() {
                                self.state.albums_exhausted = true;
                            } else {
                                // 服务端已按所选方式排序；仅“最近添加”需要
                                // 同页内按 created 时间修正顺序。
                                if self.state.albums_sort == AlbumSort::NewestAdded {
                                    albums.sort_by_key(|album| {
                                        album.created.clone().unwrap_or_default()
                                    });
                                    albums.reverse();
                                }
                                let covers: Vec<String> = albums
                                    .iter()
                                    .filter_map(|item| item.cover_art.clone())
                                    .take(80)
                                    .collect();
                                let cover_count = covers.len();
                                self.state.albums.extend(albums);
                                self.state.albums_page += 1;
                                self.preload_covers(covers);
                                log::debug!(
                                    "albums page handled in {:?}; total={} covers_queued={cover_count}",
                                    handle_start.elapsed(),
                                    self.state.albums.len(),
                                );
                            }
                        }
                        Err(error) => {
                            self.state.albums_page_failed = true;
                            self.notify_error(error);
                        }
                    }
                }
                Msg::NewestAlbums(result) => {
                    self.state.newest_albums_loading = false;
                    match result {
                        Ok(albums) => {
                            let covers: Vec<String> = albums
                                .iter()
                                .filter_map(|item| item.cover_art.clone())
                                .take(30)
                                .collect();
                            self.state.newest_albums = albums;
                            self.preload_covers(covers);
                        }
                        Err(error) => {
                            // 非关键增强：加载失败只记录日志，不阻断其余页面。
                            log::warn!("newest albums load failed: {error}");
                        }
                    }
                }
                Msg::Genres(result) => {
                    self.state.genres_loading = false;
                    match result {
                        Ok(mut genres) => {
                            // 服务器返回顺序不定，统一按名称 A→Z 展示；
                            // 写入磁盘缓存供下次启动即时展示。
                            genres.sort_by_key(|genre| genre.value.trim().to_lowercase());
                            if let Err(error) = config::save_genres_cache(&genres) {
                                log::warn!("failed to cache genres: {error}");
                            }
                            self.state.genres = genres;
                        }
                        Err(error) => self.notify_error(error),
                    }
                }
                Msg::OutputDevices(result) => {
                    self.state.output_devices_loading = false;
                    match result {
                        Ok(devices) => self.state.output_devices = devices,
                        Err(error) => self.notify_error(error),
                    }
                }
                Msg::GenreAlbums { genre, result } => {
                    self.state.genre_albums_loading = false;
                    // 只处理当前流派页的响应，忽略过期请求。
                    if self.state.current_genre.as_deref() == Some(genre.as_str()) {
                        match result {
                            Ok(albums) => {
                                if albums.is_empty() {
                                    self.state.genre_albums_exhausted = true;
                                } else {
                                    let covers: Vec<String> = albums
                                        .iter()
                                        .filter_map(|item| item.cover_art.clone())
                                        .take(80)
                                        .collect();
                                    self.state.genre_albums.extend(albums);
                                    self.state.genre_albums_page += 1;
                                    self.preload_covers(covers);
                                }
                            }
                            Err(error) => {
                                self.state.genre_albums_page_failed = true;
                                self.notify_error(error);
                            }
                        }
                    }
                }
                Msg::RecentAlbums(result) => {
                    self.state.recent_albums_loading = false;
                    match result {
                        Ok(albums) => {
                            let covers: Vec<String> = albums
                                .iter()
                                .filter_map(|item| item.cover_art.clone())
                                .take(30)
                                .collect();
                            self.state.recent_albums = albums;
                            self.preload_covers(covers);
                        }
                        Err(error) => {
                            // 非关键增强：加载失败只记录日志，不阻断其余页面。
                            log::warn!("recent albums load failed: {error}");
                        }
                    }
                }
                Msg::FrequentAlbums(result) => {
                    self.state.frequent_albums_loading = false;
                    match result {
                        Ok(albums) => {
                            let covers: Vec<String> = albums
                                .iter()
                                .filter_map(|item| item.cover_art.clone())
                                .take(30)
                                .collect();
                            self.state.frequent_albums = albums;
                            self.preload_covers(covers);
                        }
                        Err(error) => log::warn!("frequent albums load failed: {error}"),
                    }
                }
                Msg::RandomAlbums(result) => {
                    self.state.random_albums_loading = false;
                    match result {
                        Ok(albums) => {
                            let covers: Vec<String> = albums
                                .iter()
                                .filter_map(|item| item.cover_art.clone())
                                .take(30)
                                .collect();
                            self.state.random_albums = albums;
                            self.preload_covers(covers);
                        }
                        Err(error) => log::warn!("random albums load failed: {error}"),
                    }
                }
                Msg::AlbumSongs { album_id, result } => {
                    self.state.loading = false;
                    if self.state.current_album.as_ref().map(|a| a.id.as_str())
                        == Some(album_id.as_str())
                    {
                        match result {
                            Ok(mut songs) => {
                                sort_album_songs(&mut songs);
                                // 只预加载首屏封面，滚动时按需补齐，避免一次发起几百个请求。
                                let covers = songs
                                    .iter()
                                    .filter_map(|song| song.cover_art.clone())
                                    .take(40)
                                    .collect();
                                self.state.current_songs = songs;
                                self.preload_covers(covers);
                                if self.state.pending_play_album {
                                    self.state.pending_play_album = false;
                                    let songs = self.state.current_songs.clone();
                                    self.play_song_list(&songs, 0);
                                }
                            }
                            Err(error) => self.notify_error(error),
                        }
                    }
                }
                Msg::Playlists {
                    generation,
                    request,
                    result,
                } => {
                    if generation != self.playlist_ui.generation
                        || request != self.playlist_ui.list_request
                    {
                        continue;
                    }
                    self.playlist_ui.list_loading = false;
                    match result {
                        Ok(playlists) => {
                            self.state.playlists_visible = playlists.len().min(50);
                            self.state.playlists = playlists;
                        }
                        Err(error) => self.playlist_read_error(error),
                    }
                }
                Msg::PlaylistEdited { generation, result } => {
                    self.finish_playlist_edit(generation, result, window, cx);
                }
                Msg::Favorites(result) => match result {
                    Ok(favorites) => {
                        let mut favorite_ids = HashSet::new();
                        favorite_ids.extend(
                            favorites
                                .artists
                                .iter()
                                .map(|artist| FavoriteKey::new(FavoriteKind::Artist, &artist.id)),
                        );
                        favorite_ids.extend(
                            favorites
                                .albums
                                .iter()
                                .map(|album| FavoriteKey::new(FavoriteKind::Album, &album.id)),
                        );
                        favorite_ids.extend(
                            favorites
                                .songs
                                .iter()
                                .map(|song| FavoriteKey::new(FavoriteKind::Song, &song.id)),
                        );
                        for key in &self.state.pending_favorites {
                            if self.state.favorite_ids.contains(key) {
                                favorite_ids.insert(key.clone());
                            } else {
                                favorite_ids.remove(key);
                            }
                        }
                        let covers = favorites
                            .albums
                            .iter()
                            .filter_map(|album| album.cover_art.clone())
                            .chain(
                                favorites
                                    .artists
                                    .iter()
                                    .filter_map(|artist| artist.cover_art.clone()),
                            )
                            .chain(
                                favorites
                                    .songs
                                    .iter()
                                    .filter_map(|song| song.cover_art.clone()),
                            )
                            .collect();
                        self.state.favorites = favorites;
                        self.state.favorite_ids = favorite_ids;
                        self.preload_covers(covers);
                    }
                    Err(error) => self.notify_error(error),
                },
                Msg::FavoriteChanged {
                    key,
                    starred,
                    result,
                } => {
                    self.state.pending_favorites.remove(&key);
                    if let Err(error) = result {
                        if starred {
                            self.state.favorite_ids.remove(&key);
                        } else {
                            self.state.favorite_ids.insert(key);
                        }
                        let action = if starred { "add" } else { "remove" };
                        self.notify_error(format!("Failed to {action} favorite: {error}"));
                    } else {
                        self.load_favorites();
                    }
                }
                Msg::PlaylistSongs {
                    generation,
                    request,
                    playlist_id,
                    result,
                } => {
                    if generation != self.playlist_ui.generation
                        || request != self.playlist_ui.detail_request
                    {
                        continue;
                    }
                    self.playlist_ui.detail_loading = false;
                    if self.state.current_playlist.as_ref().map(|p| p.id.as_str())
                        == Some(playlist_id.as_str())
                    {
                        match result {
                            Ok(details) => {
                                self.state.current_playlist = Some(details.playlist);
                                let songs = details.songs;
                                // 只预加载首屏封面，滚动时按需补齐。
                                let covers = songs
                                    .iter()
                                    .filter_map(|song| song.cover_art.clone())
                                    .take(40)
                                    .collect();
                                self.state.playlist_songs = songs;
                                self.preload_covers(covers);
                            }
                            Err(error) => self.notify_error(error),
                        }
                    }
                }
                Msg::Search(result) => {
                    self.state.loading = false;
                    match result {
                        Ok(results) => {
                            self.state.search_results = Some(results);
                        }
                        Err(error) => self.notify_error(error),
                    }
                }
                Msg::PlayArtistSongs(result) => match result {
                    Ok(songs) => {
                        self.play_song_list(&songs, 0);
                    }
                    Err(error) => self.notify_error(error),
                },
                Msg::Lyrics { song_id, result } => {
                    if self.state.now_playing.as_ref().map(|song| song.id.as_str())
                        == Some(song_id.as_str())
                    {
                        self.state.lyrics_loading = false;
                        self.active_lyric_index = None;
                        match result {
                            Ok(lyrics) => {
                                self.state.lyrics = Some(lyrics);
                                self.state.lyrics_error = None;
                            }
                            Err(error) => {
                                self.state.lyrics = None;
                                self.state.lyrics_error = Some(error);
                            }
                        }
                    }
                }
                Msg::Cover { id, result } => {
                    if let Ok(decoded) = result {
                        if let Some(palette) = decoded.palette {
                            self.state.cover_palettes.insert(id.clone(), palette);
                        }
                        if let Some(image) = decoded.image {
                            self.state.covers.insert(id, image);
                        }
                    }
                }
            }
        }
        changed
    }
}
