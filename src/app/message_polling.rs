//! 后台 API 消息与系统托盘命令轮询。

use super::*;

impl NavidromeApp {
    pub(super) fn poll_tray_commands(
        &mut self,
        // 其他平台（Windows）通过 hwnd 还原窗口，不使用 window 句柄。
        #[cfg_attr(target_os = "windows", allow(unused_variables))] window: &mut Window,
    ) -> bool {
        let mut changed = false;
        while let Ok(command) = self.tray_rx.try_recv() {
            changed = true;
            match command {
                TrayCommand::TogglePlayback => self.toggle_playback(),
                TrayCommand::Previous => self.skip(-1),
                TrayCommand::Next => self.skip(1),
                TrayCommand::ShowWindow => {
                    // Windows 经 hwnd 还原置前；其他平台走 gpui 平台 API。
                    #[cfg(target_os = "windows")]
                    if let Some(hwnd) = self.main_hwnd {
                        show_main_window(hwnd);
                    }
                    #[cfg(not(target_os = "windows"))]
                    window.activate_window();
                }
                TrayCommand::Quit => {
                    self.quitting.store(true, Ordering::Relaxed);
                    #[cfg(target_os = "windows")]
                    if let Some(hwnd) = self.main_hwnd {
                        request_window_close(hwnd);
                    }
                    #[cfg(not(target_os = "windows"))]
                    window.remove_window();
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
                                info.display_version(),
                                self.api
                                    .as_ref()
                                    .map(|api| api.base_url().to_string())
                                    .unwrap_or_default()
                            );
                            self.state.server = Some(info);
                            self.load_open_subsonic_info();
                        }
                        Err(error) => {
                            self.state.status = t!("Connection failed").to_string();
                            self.notify_error(t!("Connection failed: %{error}", error = error));
                        }
                    }
                }
                Msg::OpenSubsonicInfo { base_url, result } => {
                    if self.api.as_ref().map(OpenSubsonicProvider::base_url)
                        != Some(base_url.as_str())
                    {
                        continue;
                    }
                    match result {
                        Ok(info) => {
                            if let Some(server) = &mut self.state.server {
                                server.merge_open_subsonic_info(info);
                                let server_name =
                                    server.app_name.as_deref().unwrap_or("Navidrome");
                                self.state.status = format!(
                                    "{server_name} {} - {}",
                                    server.display_version(),
                                    base_url
                                );
                            }
                        }
                        Err(error) => log::debug!(
                            "OpenSubsonic capability discovery unavailable; using Subsonic compatibility mode: {error}"
                        ),
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
                Msg::ArtistTopSongs {
                    generation,
                    artist_id,
                    result,
                } => {
                    if generation != self.artist_top_songs_generation
                        || self.state.current_artist.as_ref().map(|a| a.id.as_str())
                            != Some(artist_id.as_str())
                    {
                        continue;
                    }
                    self.state.artist_top_songs_loading = false;
                    match result {
                        Ok(songs) => {
                            let covers = songs
                                .iter()
                                .filter_map(|song| song.cover_art.clone())
                                .take(12)
                                .collect();
                            self.state.artist_top_songs = songs;
                            self.state.artist_top_songs_error = None;
                            self.preload_covers(covers);
                        }
                        Err(error) => {
                            self.state.artist_top_songs.clear();
                            self.state.artist_top_songs_error = Some(error);
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
                            if let Err(error) =
                                config::save_genres_cache(self.config.active_server_id(), &genres)
                            {
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
                            let featured_cover =
                                albums.first().and_then(|album| album.cover_art.clone());
                            let covers: Vec<String> = albums
                                .iter()
                                .filter_map(|item| item.cover_art.clone())
                                .skip(featured_cover.is_some() as usize)
                                .take(30)
                                .collect();
                            self.state.recent_albums = albums;
                            self.ensure_cover(featured_cover.as_deref(), true);
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
                Msg::AutoContinue {
                    generation,
                    server_id,
                    anchor_song_id,
                    queue_len,
                    result,
                } => self.finish_auto_continue(
                    generation,
                    server_id,
                    anchor_song_id,
                    queue_len,
                    result,
                ),
                Msg::DailyMix {
                    generation,
                    server_id,
                    result,
                } => self.finish_daily_mix(generation, server_id, result),
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
                            let covers = playlists
                                .iter()
                                .filter_map(|playlist| playlist.cover_art.clone())
                                .collect();
                            self.state.playlists_visible = playlists.len().min(50);
                            self.state.playlists = playlists;
                            // 歌单列表此前只渲染 coverArt，却从未请求它，因此即使
                            // Navidrome 返回了封面 id，也会永远停在默认图。
                            self.preload_covers(covers);
                        }
                        Err(error) => self.playlist_read_error(error),
                    }
                }
                Msg::PlaylistEdited { generation, result } => {
                    self.finish_playlist_edit(generation, result, window, cx);
                }
                Msg::Favorites(result) => match result {
                    Ok(favorites) => {
                        self.clear_song_multi();
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
                        let action = if starred { t!("add") } else { t!("remove") };
                        self.notify_error(t!(
                            "Failed to %{action} favorite: %{error}",
                            action = action,
                            error = error
                        ));
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
                                let mut playlist = details.playlist;
                                let songs = details.songs;
                                // 部分兼容服务器不给歌单自身 coverArt；打开详情后
                                // 用第一首带封面的曲目兜底，并回写列表项供返回时复用。
                                if playlist.cover_art.is_none() {
                                    playlist.cover_art =
                                        songs.iter().find_map(|song| song.cover_art.clone());
                                }
                                let cover = playlist.cover_art.clone();
                                if let Some(list_item) = self
                                    .state
                                    .playlists
                                    .iter_mut()
                                    .find(|item| item.id == playlist.id)
                                {
                                    list_item.cover_art = cover.clone();
                                }
                                self.state.current_playlist = Some(playlist);
                                // 只预加载首屏封面，滚动时按需补齐。
                                let covers = songs
                                    .iter()
                                    .filter_map(|song| song.cover_art.clone())
                                    .take(40)
                                    .collect();
                                self.state.playlist_songs = songs;
                                self.ensure_cover(cover.as_deref(), true);
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
                Msg::Radio {
                    generation,
                    server_id,
                    key,
                    initial_song_id,
                    result,
                } => self.finish_radio(generation, server_id, key, initial_song_id, result),
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
                    self.state.requested_covers.remove(&id);
                    if let Ok(decoded) = result {
                        let decoded_ok = decoded.image.is_some() || decoded.palette.is_some();
                        if let Some(palette) = decoded.palette {
                            self.state.cover_palettes.insert(id.clone(), palette);
                        }
                        if let Some(image) = decoded.image {
                            self.mark_cover_image_used(&image);
                            let image_id = image.id();
                            if let Some(previous) = self.state.covers.insert(id.clone(), image) {
                                if image_id != previous.id() {
                                    self.queue_cover_asset_release(previous);
                                }
                            }
                        }
                        if !decoded_ok {
                            self.remember_failed_cover(id.clone());
                        }
                        // 正在播放曲目的封面若被网格档（无调色板）抢先
                        // 解码，立刻补提一次调色板——命中封面磁盘缓存，
                        // 不再走网络。播放页与侧边栏胶囊的动态配色靠它。
                        if decoded_ok && decoded.palette.is_none() {
                            let current = self
                                .state
                                .now_playing
                                .as_ref()
                                .and_then(|song| song.cover_art.as_deref());
                            if current == Some(id.as_str()) {
                                self.ensure_cover(Some(&id), true);
                            }
                        }
                    } else {
                        // 失败缓存保持有界；避免坏封面在每次渲染时无限重试，
                        // 也避免浏览超大曲库时失败 id 永久增长。
                        self.remember_failed_cover(id);
                    }
                }
                Msg::ImportResolved { generation, result } => {
                    self.finish_import_resolve(generation, result, cx);
                }
                Msg::ImportProgress { generation, done } => {
                    self.finish_import_progress(generation, done);
                }
                Msg::ImportMatched { generation, result } => {
                    self.finish_import_match(generation, result, window, cx);
                }
            }
        }
        changed
    }
}
