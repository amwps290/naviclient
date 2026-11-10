//! Changing the browsing connection without interrupting its playback owner.

use crate::app::prelude::*;

impl NavidromeApp {
    /// Save the player under its own source; browsing changes do not finish it.
    pub(in crate::app) fn prepare_server_change(&mut self) {
        self.state.settings.bilibili_login.cancel();
        self.sync_gapless_transition();
        self.save_session();
        self.persist_loudness();
        self.flush_server_cache_writes();
    }

    /// 断开当前服务器并重建连接与视图状态（切服务器、保存设置共用）。
    pub(in crate::app) fn reconnect_active_server(&mut self) {
        self.state.reset_browsing();
        self.playlist_ui.reset();
        self.import_ui.reset();
        self.state.albums_sort = self.config.albums_sort;
        self.state.artists.set_sort(self.config.artists_sort);

        if self.browsing_bilibili() {
            self.api = self
                .config
                .active_source
                .as_ref()
                .and_then(|id| self.available_source(id));
            self.state.view = View::Playlists;
            self.prune_source_covers();
            if self.api.is_some() {
                if self.playback_source.is_none() {
                    self.playback_source = self.api.clone();
                    self.load_active_server_cache();
                }
                self.refresh_library();
            } else {
                self.state.status = match self
                    .config
                    .active_source
                    .as_ref()
                    .and_then(|source| self.bilibili_accounts.get(source))
                {
                    Some(crate::app::settings::bilibili_state::AccountStatus::Failed(error)) => {
                        error.message().to_string()
                    }
                    Some(crate::app::settings::bilibili_state::AccountStatus::Checking {
                        ..
                    }) => t!("Verifying Bilibili account…").to_string(),
                    _ => t!("Sign in to Bilibili").to_string(),
                };
            }
            return;
        }

        // 空白条目（刚新增还没填）不是错误：只清空连接，等用户保存地址。
        let configured = self
            .config
            .active_source
            .as_ref()
            .is_some_and(|source| source.provider == crate::source::ProviderKind::OpenSubsonic)
            && self
                .config
                .active_profile()
                .is_some_and(|profile| profile.is_configured());
        if !configured {
            self.api = None;
            self.prune_source_covers();
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
        let route = match self
            .config
            .active_source
            .as_ref()
            .map(|source| self.config.route_for(source))
            .transpose()
        {
            Ok(route) => route.unwrap_or_default(),
            Err(_) => {
                self.api = None;
                self.notify_error(t!(
                    "The selected proxy is missing. Choose a proxy or direct connection."
                ));
                self.state.settings.open = true;
                return;
            }
        };
        match MusicSource::opensubsonic_with_route(
            self.config.active_server.as_deref().unwrap_or_default(),
            &self.config.server_url,
            &self.config.username,
            &password,
            route,
        ) {
            Ok(api) => {
                self.api = Some(api);
                self.prune_source_covers();
                if self.playback_source.is_none() {
                    self.playback_source = self.api.clone();
                    self.load_playback_source_cache();
                }
                self.load_browsing_source_cache();
                self.refresh_library();
            }
            Err(error) => {
                self.api = None;
                self.prune_source_covers();
                self.state.status = t!("Invalid server URL").to_string();
                self.notify_error(format!("{error:#}"));
                self.state.settings.open = true;
            }
        }
    }
}
