//! 后台 API 消息与系统托盘命令轮询。

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn poll_tray_commands(
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
                    self.save_playback_before_exit();
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

    pub(in crate::app) fn poll_messages(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut changed = false;
        if let Some(error) = self.lyrics_cache.take_error() {
            self.notify_error(t!("Could not save lyrics: %{error}", error = error));
            changed = true;
        }
        while let Ok(message) = self.rx.try_recv() {
            changed = true;
            match message {
                Msg::BilibiliLogin { request, update } => {
                    self.finish_bilibili_login(request, update)
                }
                Msg::BilibiliAccount {
                    request,
                    source,
                    result,
                } => self.finish_bilibili_account(request, source, result),
                Msg::BilibiliRemoved { source, result } => {
                    self.finish_bilibili_removal(source, result)
                }
                Msg::ProxyProbed {
                    proxy,
                    request,
                    result,
                } => self.finish_proxy_probe(proxy, request, result),
                Msg::PlaybackResolved { request, result } => {
                    self.finish_playback_resolution(request, result)
                }
                Msg::PreloadResolved { request, result } => {
                    self.finish_preload_resolution(request, result)
                }
                Msg::Ping { request, result } => self.finish_ping(request, result),
                Msg::OpenSubsonicInfo { request, result } => {
                    self.finish_open_subsonic_info(request, result)
                }
                Msg::Artists { request, result } => self.finish_artists(request, result),
                Msg::ArtistAlbums { request, result } => {
                    self.finish_artist_albums(request, result);
                }
                Msg::ArtistTopSongs { request, result } => {
                    self.finish_artist_top_songs(request, result);
                }
                Msg::Albums { request, result } => self.handle_album_page(request, result),
                Msg::HomeAlbums {
                    section,
                    request,
                    result,
                } => {
                    self.finish_home_albums(section, request, result);
                }
                Msg::Genres { request, result } => self.finish_genres(request, result),
                Msg::OutputDevices { request, result } => {
                    self.finish_output_devices(request, result)
                }
                Msg::GenreAlbums { request, result } => {
                    self.handle_genre_album_page(request, result);
                }
                Msg::AutoContinue { request, result } => self.finish_auto_continue(request, result),
                Msg::DailyMix { request, result } => self.finish_daily_mix(request, result),
                Msg::AlbumSongs { request, result } => {
                    self.finish_album_details(request, result);
                }
                Msg::Playlists { request, result } => {
                    self.finish_playlists(request, result);
                }
                Msg::PlaylistEdited { request, result } => {
                    self.finish_playlist_edit(request, result, window, cx);
                }
                Msg::Favorites { request, result } => {
                    self.finish_favorites(request, result);
                }
                Msg::FavoriteChanged { change, result } => {
                    self.finish_favorite_change(change, result);
                }
                Msg::PlaylistSongs { request, result } => {
                    self.finish_playlist_detail(request, result);
                }
                Msg::Search { request, result } => self.finish_search(request, result),
                Msg::LibraryPlayback { request, result } => {
                    self.finish_library_playback(request, result);
                }
                Msg::Radio { request, result } => self.finish_radio(request, result),
                Msg::Lyrics { request, result } => {
                    self.state.lyrics.finish(request, result);
                }
                Msg::LyricsProgress { request, status } => {
                    self.state.lyrics.progress(request, status);
                }
                Msg::LyricsSearch { request, result } => {
                    self.finish_lyrics_search(request, result);
                }
                Msg::Cover { request, result } => {
                    self.finish_cover(request, result);
                }
                Msg::ImportResolved { request, result } => {
                    self.finish_import_resolve(request, result, cx);
                }
                Msg::ImportProgress { request, done } => {
                    self.finish_import_progress(request, done);
                }
                Msg::ImportMatched { request, result } => {
                    self.finish_import_match(request, result, window, cx);
                }
            }
        }
        // A finished login or a closed settings overlay ends its dialog.
        self.sync_settings_dialog(window, cx);
        self.sync_lyrics_dialog(window, cx);
        changed
    }
}
