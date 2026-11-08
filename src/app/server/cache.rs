//! Browsing caches and playback caches have separate source owners.

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn load_active_server_cache(&mut self) {
        self.load_playback_source_cache();
        self.load_browsing_source_cache();
        let Some(api) = self.playback_source.as_ref() else {
            return;
        };
        let source = api.id().clone();
        if let Some(session) = self
            .server_cache_writer
            .pending_source_session(&source)
            .filter(|session| {
                session.matches_source(
                    &source,
                    self.playback_source.as_ref().map(MusicSource::base_url),
                )
            })
            .or_else(|| {
                config::load_source_session(
                    &source,
                    self.playback_source.as_ref().map(MusicSource::base_url),
                )
            })
        {
            self.restore_session(session);
        }
    }

    pub(in crate::app) fn load_playback_source_cache(&mut self) {
        let Some(api) = self.playback_source.as_ref() else {
            return;
        };
        let source = api.id().clone();
        self.session
            .bind_source(source.clone(), Some(api.base_url()));
        let server_id = self.session.server_id().map(str::to_string);
        self.state.loudness.restore(
            server_id.as_deref(),
            self.server_cache_writer
                .pending_source_loudness(&source)
                .unwrap_or_else(|| config::load_source_loudness(&source)),
        );
        self.state.listening.restore(
            server_id.as_deref(),
            self.server_cache_writer
                .pending_source_history(&source)
                .unwrap_or_else(|| config::load_source_history(&source)),
        );
        let history_songs = self
            .state
            .listening
            .recent_songs()
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>();
        for song in history_songs {
            self.ensure_song_cover(&song, false);
        }
    }

    pub(in crate::app) fn load_browsing_source_cache(&mut self) {
        let Some(api) = self.api.as_ref() else { return };
        if api.id().provider != crate::source::ProviderKind::OpenSubsonic {
            return;
        }
        let server_id = Some(api.id().profile_id.clone());
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
    }
}
