//! Saving the outgoing server and resetting playback and library state on reconnect.

use super::*;

impl NavidromeApp {
    /// Resolve any already committed gapless transition while the old API and
    /// profile still agree, then persist that server's playback and caches.
    pub(super) fn prepare_server_change(&mut self) {
        self.cancel_next_preload();
        self.finish_listening_event(false);
        self.save_session();
        self.persist_loudness();
        self.flush_server_cache_writes();
    }

    /// 断开当前服务器并重建连接与视图状态（切服务器、保存设置共用）。
    pub(super) fn reconnect_active_server(&mut self) {
        self.cancel_recommendation_tasks();
        self.cancel_next_preload();
        self.finish_listening_event(false);
        self.audio.stop();
        self.retire_server_covers();
        self.state = AppState::default();
        self.playlist_ui.reset();
        self.import_ui.reset();
        self.session.bind(None, None);
        self.reconnect.cancel();
        // 上一台服务器的曲目不应继续出现在“正在播放”与通知里。
        self.desktop.reset_track();
        self.track_notifier.cancel_pending();
        self.state.playback_modes = self.config.playback_modes;
        self.state.albums_sort = self.config.albums_sort;
        self.state.artists.set_sort(self.config.artists_sort);
        // 空白条目（刚新增还没填）不是错误：只清空连接，等用户保存地址。
        let configured = self
            .config
            .active_profile()
            .is_some_and(|profile| profile.is_configured());
        if !configured {
            self.api = None;
            self.state.status = t!("Enter the server address and account").to_string();
            self.state.settings.open = true;
            return;
        }
        // 已迁入系统凭据库的条目用解析后的密码连接。
        let password = self
            .config
            .active_profile()
            .map(|profile| profile.resolved_password())
            .unwrap_or_default();
        match OpenSubsonicProvider::new(&self.config.server_url, &self.config.username, &password) {
            Ok(api) => {
                self.api = Some(api);
                self.load_active_server_cache();
                self.refresh_library();
            }
            Err(error) => {
                self.api = None;
                self.state.status = t!("Invalid server URL").to_string();
                self.notify_error(format!("{error:#}"));
                self.state.settings.open = true;
            }
        }
    }
}
