//! Restoring active server caches and prefetching their artwork.

use crate::app::prelude::*;

impl NavidromeApp {
    /// 加载当前服务器作用域下的播放会话、响度、流派、收听历史与 Daily Mix。
    /// 启动与切换服务器时都走这里；切换时调用方已清空 `state`。
    pub(in crate::app) fn load_active_server_cache(&mut self) {
        let server_id = self.config.active_server_id().map(str::to_string);
        self.session.bind(
            server_id.as_deref(),
            self.api.as_ref().map(|api| api.base_url()),
        );
        self.state.loudness.restore(
            server_id.as_deref(),
            self.server_cache_writer
                .pending_loudness(server_id.as_deref())
                .unwrap_or_else(|| config::load_loudness(server_id.as_deref())),
        );
        self.state.listening.restore(
            server_id.as_deref(),
            self.server_cache_writer
                .pending_history(server_id.as_deref())
                .unwrap_or_else(|| config::load_listening_history(server_id.as_deref())),
        );
        let history_covers = self
            .state
            .listening
            .recent_songs()
            .iter()
            .filter_map(|song| song.cover_art.clone())
            .take(40)
            .collect();
        self.preload_covers(history_covers);
        let day = crate::recommendation::current_day_key();
        let cached_daily_mix = self
            .server_cache_writer
            .pending_daily_mix(server_id.as_deref())
            .or_else(|| config::load_daily_mix(server_id.as_deref()));
        self.state.daily_mix.restore(
            cached_daily_mix,
            server_id.as_deref().unwrap_or_default(),
            &day,
        );
        let daily_accent_cover = self
            .state
            .daily_mix
            .snapshot()
            .iter()
            .flat_map(|snapshot| snapshot.tracks.iter())
            .find_map(|track| track.song.cover_art.clone());
        let daily_covers = self
            .state
            .daily_mix
            .snapshot()
            .iter()
            .flat_map(|snapshot| snapshot.tracks.iter())
            .filter_map(|track| track.song.cover_art.clone())
            .take(30)
            .collect();
        self.ensure_cover(daily_accent_cover.as_deref(), true);
        self.preload_covers(daily_covers);
        // 流派是服务端重查询（全库聚合）：先用磁盘缓存即时展示，后台刷新后覆盖。
        self.state.genres.restore(
            self.server_cache_writer
                .pending_genres(server_id.as_deref())
                .or_else(|| config::load_genres_cache(server_id.as_deref())),
        );

        if let Some(session) = self
            .server_cache_writer
            .pending_session(server_id.as_deref())
            .filter(|session| session.matches_server(self.config.server_url.trim()))
            .or_else(|| config::load_session(server_id.as_deref(), self.config.server_url.trim()))
        {
            self.restore_session(session);
        }
    }
}
