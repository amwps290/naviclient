//! Application orchestration for offline pinning, offline browsing/playback mode, and outbox sync.

use crate::app::playback::display::format_file_size;
use crate::app::prelude::*;
use crate::offline::{
    download_track, OfflineDownloadOutcome, OfflineStats, OfflineTrackStatus, PendingSyncOp,
};
use crate::source::{HttpResource, MediaKey, PlaybackResource, ProviderKind, SourceId};

impl NavidromeApp {
    /// Whether offline mode is active for the currently browsed network source.
    /// LocalFolder sources are inherently offline and never restricted by this flag.
    pub(in crate::app) fn offline_mode_active(&self) -> bool {
        self.config.offline_mode
            && self
                .api
                .as_ref()
                .is_some_and(|api| api.id().provider != ProviderKind::LocalFolder)
    }

    pub(in crate::app) fn offline_mode_for_source(&self, source: &SourceId) -> bool {
        self.config.offline_mode && source.provider != ProviderKind::LocalFolder
    }

    pub(in crate::app) fn set_offline_mode(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.config.offline_mode == enabled {
            return;
        }
        self.config.offline_mode = enabled;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Config save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        if enabled {
            self.notify_success(t!(
                "Offline mode enabled — showing downloaded tracks only."
            ));
        } else {
            self.flush_offline_outbox();
            self.notify_success(t!("Back online — refreshing library."));
        }
        self.refresh_library();
        cx.notify();
    }

    pub(in crate::app) fn song_media_key(&self, song: &Song) -> Option<MediaKey> {
        song.media_key().or_else(|| {
            self.source_for_song(song)
                .map(|api| MediaKey::new(api.id().clone(), &song.id))
        })
    }

    pub(in crate::app) fn song_offline_status(&self, song: &Song) -> OfflineTrackStatus {
        let Some(media) = self.song_media_key(song) else {
            return OfflineTrackStatus::NotPinned;
        };
        if media.source.provider == ProviderKind::LocalFolder {
            return OfflineTrackStatus::NotPinned;
        }
        self.offline_store.status(&media)
    }

    pub(in crate::app) fn song_is_pinned_offline(&self, song: &Song) -> bool {
        self.song_offline_status(song) == OfflineTrackStatus::Ready
    }

    /// Resolve a song to its protected offline local file if pinned on disk.
    pub(in crate::app) fn offline_playback_resource(
        &self,
        song: &Song,
        source: &MusicSource,
    ) -> Option<PlaybackResource> {
        let media = MediaKey::new(source.id().clone(), &song.id);
        let path = self.offline_store.audio_path(&media)?;
        let mime_hint = crate::source::local::mime_for_path(&path)
            .map(str::to_string)
            .or_else(|| song.content_type.clone());
        let http = HttpResource::local_file(path).with_mime_hint(mime_hint);
        let duration = song
            .duration
            .and_then(|secs| u64::try_from(secs).ok())
            .filter(|secs| *secs > 0)
            .map(Duration::from_secs);
        Some(PlaybackResource {
            media,
            representation: "offline".to_string(),
            http,
            backups: Vec::new(),
            quality: crate::source::AudioQuality {
                codec: song.suffix.as_deref().map(|s| s.to_ascii_uppercase()),
                bandwidth: song
                    .bit_rate
                    .and_then(|b| u64::try_from(b).ok())
                    .map(|b| b.saturating_mul(1_000)),
                sample_rate: None,
                channels: None,
            },
            duration,
        })
    }

    pub(in crate::app) fn offline_stats(&self) -> OfflineStats {
        self.offline_store.stats(None)
    }

    pub(in crate::app) fn offline_downloading_count(&self) -> usize {
        self.offline_store.downloading_count()
    }

    pub(in crate::app) fn sync_offline_store_dir(&mut self) {
        let dir = config::offline_audio_dir(&self.config);
        self.offline_store.set_dir(dir.clone());
        self.offline_outbox.set_dir(&dir);
    }

    pub(in crate::app) fn pin_song_offline(&mut self, song: Song) {
        self.pin_songs_offline(vec![song], None);
    }

    pub(in crate::app) fn pin_songs_offline(
        &mut self,
        songs: Vec<Song>,
        playlist_snapshot: Option<Playlist>,
    ) {
        if self.config.offline_mode {
            self.notify_warning(t!("Disable offline mode to download new tracks."));
            return;
        }
        let Some(api) = self.api.clone() else {
            return;
        };
        if api.id().provider == ProviderKind::LocalFolder {
            return;
        }
        if let Some(playlist) = playlist_snapshot {
            let _ = self
                .offline_store
                .record_playlist_snapshot(api.id().clone(), playlist, &songs);
        }
        let mut queued = 0usize;
        for song in songs {
            let media = MediaKey::new(api.id().clone(), &song.id);
            if !self.offline_store.mark_downloading(media) {
                continue;
            }
            queued += 1;
            let api = api.clone();
            let quality = self.config.transcoding_quality;
            let config = self.config.clone();
            let offline_dir = self.offline_store.dir().to_path_buf();
            let audio_cache_dir = config::audio_cache_dir(&self.config);
            let slots = self.offline_slots.clone();
            let tx = self.tx.clone();
            self.spawn_future(async move {
                let _permit = slots.acquire_owned().await;
                let outcome = download_track(
                    api,
                    song,
                    quality,
                    config,
                    offline_dir,
                    audio_cache_dir,
                )
                .await;
                let _ = tx.send(Msg::OfflineDownloaded {
                    outcome: Box::new(outcome),
                });
            });
        }
        if queued > 0 {
            self.notify_success(t!(
                "Queued %{count} track(s) for offline download",
                count = queued
            ));
        }
    }

    pub(in crate::app) fn unpin_song_offline(&mut self, song: &Song) {
        let Some(media) = self.song_media_key(song) else {
            return;
        };
        match self.offline_store.remove_track(&media) {
            Ok(true) => {
                self.notify_success(t!("Removed offline copy of “%{title}”", title = song.title));
                if self.offline_mode_active() {
                    self.refresh_library();
                }
            }
            Ok(false) => {}
            Err(error) => {
                self.notify_error(t!(
                    "Could not remove offline file: %{error}",
                    error = format!("{error:#}")
                ));
            }
        }
    }

    pub(in crate::app) fn unpin_songs_offline(&mut self, songs: &[Song]) {
        let keys: Vec<MediaKey> = songs
            .iter()
            .filter_map(|song| self.song_media_key(song))
            .collect();
        match self.offline_store.remove_tracks(&keys) {
            Ok(count) if count > 0 => {
                self.notify_success(t!("Removed %{count} offline track(s)", count = count));
                if self.offline_mode_active() {
                    self.refresh_library();
                }
            }
            Ok(_) => {}
            Err(error) => {
                self.notify_error(t!(
                    "Could not remove offline file: %{error}",
                    error = format!("{error:#}")
                ));
            }
        }
    }

    pub(in crate::app) fn clear_all_offline_action(&mut self, cx: &mut Context<Self>) {
        match self.offline_store.clear_all() {
            Ok((count, bytes)) => {
                self.notify_success(t!(
                    "Removed %{count} offline tracks (%{size})",
                    count = count,
                    size = format_file_size(bytes)
                ));
                if self.offline_mode_active() {
                    self.refresh_library();
                }
            }
            Err(error) => {
                self.notify_error(t!(
                    "Could not remove offline file: %{error}",
                    error = format!("{error:#}")
                ));
            }
        }
        cx.notify();
    }

    pub(in crate::app) fn finish_offline_download(&mut self, outcome: OfflineDownloadOutcome) {
        match outcome.result {
            Ok(entry) => {
                let title = entry.song.title.clone();
                if let Err(error) = self
                    .offline_store
                    .finish_download(&outcome.media, Some(entry))
                {
                    self.notify_error(t!(
                        "Could not save offline track: %{error}",
                        error = format!("{error:#}")
                    ));
                } else if self.offline_store.downloading_count() == 0 {
                    self.notify_success(t!(
                        "Offline download complete: %{title}",
                        title = title
                    ));
                }
            }
            Err(error) => {
                let _ = self.offline_store.finish_download(&outcome.media, None);
                self.notify_error(t!("Offline download failed: %{error}", error = error));
            }
        }
    }

    pub(in crate::app) fn queue_offline_scrobble(&mut self, source: SourceId, song_id: String) {
        let _ = self
            .offline_outbox
            .push(PendingSyncOp::Scrobble { source, song_id });
    }

    pub(in crate::app) fn queue_offline_favorite(
        &mut self,
        source: SourceId,
        key: &FavoriteKey,
        starred: bool,
    ) {
        let _ = self
            .offline_outbox
            .push(PendingSyncOp::favorite(source, key, starred));
    }

    pub(in crate::app) fn flush_offline_outbox(&mut self) {
        if self.config.offline_mode || self.offline_outbox.len() == 0 {
            return;
        }
        let Some(api) = self.api.clone() else {
            return;
        };
        let ops = self.offline_outbox.take_for_source(api.id());
        if ops.is_empty() {
            return;
        }
        self.spawn_future(async move {
            for op in ops {
                match op {
                    PendingSyncOp::Scrobble { song_id, .. } => {
                        if let Err(error) = api.scrobble(&song_id, true).await {
                            log::warn!("offline scrobble replay failed: {error:#}");
                        }
                    }
                    PendingSyncOp::SetFavorite {
                        kind, id, starred, ..
                    } => {
                        let key = FavoriteKey::new(kind.into(), id);
                        if let Err(error) = api.set_favorite(&key, starred).await {
                            log::warn!("offline favorite replay failed: {error:#}");
                        }
                    }
                }
            }
        });
    }

    /// Populate all browsing views from the protected offline manifest without network I/O.
    pub(in crate::app) fn load_offline_library(&mut self) {
        let Some(source) = self.api.as_ref().map(|api| api.id().clone()) else {
            return;
        };
        let stats = self.offline_store.stats(Some(&source));
        let mut info = ServerInfo {
            app_name: Some(t!("Offline mode").to_string()),
            server_version: Some(t!("%{count} tracks", count = stats.track_count).to_string()),
            protocol_version: "offline".into(),
            extensions_discovered: true,
            ..Default::default()
        };
        info.refresh_capabilities();
        let ping_req = self.state.connection.begin(&mut self.library_requests);
        let _ = self
            .state
            .connection
            .complete_ping(ping_req, Ok(info), &mut self.library_requests);
        self.state.status = t!(
            "Offline mode · %{count} tracks",
            count = stats.track_count
        )
        .to_string();

        let artists = self.offline_store.artists(&source);
        let req = self.state.artists.begin(&mut self.library_requests);
        self.finish_artists(req, Ok(artists));

        self.state.albums.reset();
        let albums = self.offline_store.albums(&source, self.state.albums_sort);
        if let Some(req) = self.state.albums.begin(&mut self.album_page_requests) {
            self.handle_album_page(req, Ok(albums));
        }

        let genres = self.offline_store.genres(&source);
        let req = self.state.genres.begin_list(&mut self.library_requests);
        self.finish_genres(req, Ok(genres));

        let playlists = self.offline_store.playlists(&source);
        let req = self.state.playlists.begin_list(&mut self.library_requests);
        self.finish_playlists(req, Ok(playlists));

        let newest = self.offline_store.albums(&source, AlbumSort::NewestAdded);
        for section in HomeAlbumSection::ALL {
            let req = self
                .state
                .home_albums
                .begin(section, &mut self.library_requests);
            self.finish_home_albums(section, req, Ok(newest.clone()));
        }
    }
}
