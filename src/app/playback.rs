//! 队列播放推进、播放历史、Scrobble、预加载和无缝切歌控制。

use super::*;

impl NavidromeApp {
    pub(super) fn play_queue_index(&mut self, index: usize) {
        // Explicit queue selection starts a new playback occurrence even when
        // it chooses the same song/index again. Reconnect and session resume
        // call `play_queue_index_at` directly so they do not duplicate History.
        self.finish_listening_event(false);
        self.play_queue_index_at(index, Duration::ZERO);
    }

    pub(super) fn play_queue_index_at(&mut self, index: usize, start_position: Duration) {
        self.freeze_next_preload();
        let Some(song) = self.state.queue.get(index).cloned() else {
            return;
        };
        let preload_id = self
            .next_preload
            .take()
            .filter(|preload| preload.index == index && start_position.is_zero())
            .map(|preload| preload.id);
        self.resume_position = None;
        log::info!(
            "queue playback selected; index={index} song_id={} title={:?} suffix={:?} declared_bytes={:?}",
            song.id,
            song.title,
            song.suffix,
            song.size
        );
        // Opus 等本地无法解码的音源在 Original 档位自动降级为服务端转码；
        // URL、缓存键、界面音质胶囊、无缝衔接都用同一个有效音质，保持一致。
        let configured = self.config.transcoding_quality;
        let quality = TranscodingQuality::effective_for(
            configured,
            song.suffix.as_deref(),
            song.content_type.as_deref(),
        );
        if quality != configured {
            log::info!(
                "transcode fallback engaged; song_id={} suffix={:?} {} -> {} (the local decoder chain cannot decode this source)",
                song.id,
                song.suffix,
                configured.label(),
                quality.label()
            );
        }
        self.select_playing(index, &song, quality);
        let Some(api) = self.api.clone() else {
            self.notify_error(t!("Configure a server before playing"));
            return;
        };
        let max_bit_rate = quality.max_bit_rate();
        log::info!(
            "stream profile selected; song_id={} quality={} max_bit_rate_kbps={max_bit_rate:?}",
            song.id,
            quality.label()
        );
        match api.stream_url(&song.id, max_bit_rate) {
            Ok(url) => {
                let duration = song
                    .duration
                    .and_then(|seconds| u64::try_from(seconds).ok())
                    .map(Duration::from_secs);
                let cache_key = format!(
                    "{}:{}:profile={}",
                    api.base_url(),
                    song.id,
                    quality.cache_profile()
                );
                self.state.now_playing_quality = Some(quality);
                let gain = self.song_gain(&song);
                log::debug!(
                    "volume gain song={} mode={:?} server_track_gain={:?} server_album_gain={:?} measured={:?} factor={gain:.3}",
                    song.id,
                    self.config.volume_normalization,
                    song.replay_gain_track_gain,
                    song.replay_gain_album_gain,
                    self.state.loudness_cache.get(&song.id).copied(),
                );
                self.audio
                    .play(url, cache_key, duration, gain, start_position, preload_id);
                self.report_now_playing(&song, start_position);
            }
            Err(error) => self.notify_error(t!(
                "Stream request failed: %{error}",
                error = format!("{error:#}")
            )),
        }
    }

    /// 通知 Navidrome 当前正在播放的歌曲（每首最多发送一次，不阻塞播放）。
    pub(super) fn report_now_playing(&mut self, song: &Song, position: Duration) {
        if self.state.now_playing_reported.as_deref() == Some(song.id.as_str()) {
            return;
        }
        self.state.now_playing_reported = Some(song.id.clone());
        let Some(api) = self.api.clone() else { return };
        let song_id = song.id.clone();
        let time_secs = position.as_secs() as u32;
        self.spawn_future(async move {
            if let Err(error) = api.update_now_playing(&song_id, time_secs).await {
                log::warn!("updateNowPlaying failed (non-fatal): {error:#}");
            }
        });
    }

    /// 当前歌曲播放达到有效阈值后，向 Navidrome 发送一次 Scrobble（每首一次）。
    pub(super) fn maybe_scrobble_current(&mut self) {
        let already_scrobbled = self
            .state
            .scrobbled_song_id
            .as_ref()
            .zip(self.state.now_playing.as_ref())
            .is_some_and(|(scrobbled, playing)| scrobbled == &playing.id);
        if already_scrobbled {
            return;
        }
        let Some(song_id) = self.state.now_playing.as_ref().map(|song| song.id.clone()) else {
            return;
        };
        let playback = self.audio.state();
        if !playback.active || playback.paused {
            return;
        }
        if !should_scrobble(playback.position, playback.duration) {
            return;
        }
        self.state.scrobbled_song_id = Some(song_id.clone());
        let Some(api) = self.api.clone() else { return };
        self.spawn_future(async move {
            if let Err(error) = api.scrobble(&song_id, true).await {
                log::warn!("scrobble failed (non-fatal): {error:#}");
            }
        });
    }

    /// 自然播放结束时，若尚未提交 Scrobble 则提交（完整播放一定有效）。
    pub(super) fn ensure_scrobble_on_end(&mut self) {
        let Some(song) = self.state.now_playing.clone() else {
            return;
        };
        if self.state.scrobbled_song_id.as_deref() == Some(song.id.as_str()) {
            return;
        }
        self.state.scrobbled_song_id = Some(song.id.clone());
        let Some(api) = self.api.clone() else { return };
        let song_id = song.id.clone();
        self.spawn_future(async move {
            if let Err(error) = api.scrobble(&song_id, true).await {
                log::warn!("scrobble on end failed (non-fatal): {error:#}");
            }
        });
    }

    /// Observe actual playback progress for the server-scoped local history.
    /// Large position jumps are seeks and do not count as listened time.
    pub(super) fn poll_listening_history(&mut self) {
        let Some(song) = self.state.now_playing.clone() else {
            if self.listening_song_id.is_some() {
                self.finish_listening_event(false);
            }
            return;
        };
        let playback = self.audio.state();
        if self.listening_song_id.as_deref() != Some(song.id.as_str()) {
            self.finish_listening_event(false);
            if !playback.active {
                return;
            }
            let started_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            self.state
                .listening_history
                .record_started(song.clone(), started_at);
            self.state.playback_history.record(song.clone());
            self.ensure_cover(song.cover_art.as_deref(), false);
            self.listening_song_id = Some(song.id);
            self.listening_last_position = playback.position;
            self.listening_accumulated = Duration::ZERO;
            self.last_listening_history_save = std::time::Instant::now();
            self.persist_listening_history();
            return;
        }

        if playback.active && !playback.paused {
            self.listening_accumulated +=
                listening_position_delta(self.listening_last_position, playback.position);
        }
        self.listening_last_position = playback.position;

        if self.last_listening_history_save.elapsed() >= Duration::from_secs(15) {
            self.update_listening_progress(false);
            self.persist_listening_history();
            self.last_listening_history_save = std::time::Instant::now();
        }
    }

    pub(super) fn update_listening_progress(&mut self, completed: bool) {
        let Some(song_id) = self.listening_song_id.as_deref() else {
            return;
        };
        let listened_seconds =
            u32::try_from(self.listening_accumulated.as_secs()).unwrap_or(u32::MAX);
        self.state
            .listening_history
            .update_latest(song_id, listened_seconds, completed);
    }

    pub(super) fn finish_listening_event(&mut self, completed: bool) {
        if self.listening_song_id.is_none() {
            return;
        }
        self.update_listening_progress(completed);
        self.persist_listening_history();
        self.listening_song_id = None;
        self.listening_last_position = Duration::ZERO;
        self.listening_accumulated = Duration::ZERO;
    }

    pub(super) fn persist_listening_history(&self) {
        let server_id = self.config.active_server_id();
        if let Err(error) = config::save_listening_history(server_id, &self.state.listening_history)
        {
            log::warn!("listening history save failed: {error:#}");
        }
    }

    /// 计算该歌曲的响度补偿增益：优先用服务器 ReplayGain，缺失时用本地测量缓存，都没有则 1.0。
    pub(super) fn song_gain(&self, song: &Song) -> f32 {
        match self.config.volume_normalization {
            VolumeNormalization::Off => 1.0,
            VolumeNormalization::Track => {
                if song.replay_gain_track_gain.is_some() {
                    replay_gain_factor(
                        VolumeNormalization::Track,
                        song.replay_gain_track_gain,
                        song.replay_gain_track_peak,
                        None,
                        None,
                    )
                } else if let Some(&(rms_db, peak)) = self.state.loudness_cache.get(&song.id) {
                    loudness_gain_factor(rms_db, peak)
                } else {
                    1.0
                }
            }
            VolumeNormalization::Album => {
                if song.replay_gain_album_gain.is_some() {
                    replay_gain_factor(
                        VolumeNormalization::Album,
                        None,
                        None,
                        song.replay_gain_album_gain,
                        song.replay_gain_album_peak,
                    )
                } else if let Some(&(rms_db, peak)) = self.state.loudness_cache.get(&song.id) {
                    loudness_gain_factor(rms_db, peak)
                } else {
                    1.0
                }
            }
        }
    }

    /// 自然结束时把本地测量的歌曲响度写入缓存并持久化，供后续播放做音量补偿。
    pub(super) fn cache_measured_loudness(&mut self, loudness_db: f32, loudness_peak: f32) {
        if self.config.volume_normalization == VolumeNormalization::Off {
            return;
        }
        let Some(song_id) = self.state.now_playing.as_ref().map(|song| song.id.clone()) else {
            return;
        };
        if loudness_db == 0.0 || !loudness_db.is_finite() {
            return;
        }
        self.state
            .loudness_cache
            .insert(song_id.clone(), (loudness_db, loudness_peak));
        let server_id = self.config.active_server_id().map(str::to_string);
        if let Err(error) = config::save_loudness(server_id.as_deref(), &self.state.loudness_cache)
        {
            log::warn!("loudness cache save failed: {error:#}");
        }
        log::debug!(
            "measured loudness cached song={} rms_db={:.1} peak={:.3}",
            song_id,
            loudness_db,
            loudness_peak,
        );
    }

    pub(super) fn random_shuffle_next(&self) -> usize {
        let len = self.state.queue.len();
        let current = self.state.queue_index.unwrap_or(0);
        if len <= 1 {
            return current;
        }
        let recent: HashSet<usize> = self
            .state
            .shuffle_history
            .recent((len - 1).min(8))
            .collect();
        let mut candidates: Vec<usize> = (0..len).filter(|index| !recent.contains(index)).collect();
        if candidates.is_empty() {
            candidates = (0..len).filter(|index| *index != current).collect();
        }
        candidates
            .choose(&mut thread_rng())
            .copied()
            .unwrap_or(current)
    }

    pub(super) fn cancel_next_preload(&mut self) {
        if let Some(event) = self.audio.cancel_preload() {
            self.accept_gapless_transition(event);
        }
        self.next_preload = None;
    }

    pub(super) fn freeze_next_preload(&mut self) {
        if let Some(event) = self.audio.freeze_preload() {
            self.accept_gapless_transition(event);
        }
    }

    pub(super) fn sync_gapless_transition(&mut self) {
        if let Some(event) = self.audio.take_transition() {
            self.accept_gapless_transition(event);
        }
    }

    pub(super) fn select_playing(
        &mut self,
        index: usize,
        song: &Song,
        quality: TranscodingQuality,
    ) {
        if self.listening_song_id.is_some()
            && (self.state.queue_index != Some(index)
                || self
                    .state
                    .now_playing
                    .as_ref()
                    .map(|current| current.id.as_str())
                    != Some(song.id.as_str()))
        {
            self.finish_listening_event(false);
        }
        self.resume_position = None;
        self.state.queue_index = Some(index);
        self.state.now_playing = Some(song.clone());
        self.state.now_playing_quality = Some(quality);
        self.state.ended_handled = false;
        self.state.now_playing_reported = None;
        self.state.scrobbled_song_id = None;
        self.ensure_cover(song.cover_art.as_deref(), true);
        self.load_lyrics(song);
    }

    pub(super) fn accept_gapless_transition(&mut self, event: TrackTransition) {
        let Some(next) = self.next_preload.take().filter(|next| next.id == event.id) else {
            log::error!("missing UI reservation for gapless transition {}", event.id);
            return;
        };
        let Some(song) = self.state.queue.get(next.index).cloned() else {
            return;
        };
        self.ensure_scrobble_on_end();
        self.finish_listening_event(true);
        self.cache_measured_loudness(event.loudness_db, event.loudness_peak);
        if self.state.playback_modes.shuffle
            && self.state.shuffle_history.restore_forward().is_none()
        {
            self.state.shuffle_history.advance(next.index);
        }
        self.select_playing(next.index, &song, next.quality);
        self.report_now_playing(&song, Duration::ZERO);
    }

    pub(super) fn seek_playback(&mut self, position: Duration) {
        self.cancel_next_preload();
        self.audio.seek(position);
    }

    pub(super) fn next_queue_index(&self, natural_end: bool) -> Option<usize> {
        next_queue_index(
            self.state.playback_modes,
            self.state.queue_index?,
            self.state.queue.len(),
            natural_end,
            self.state.shuffle_history.forward.last().copied(),
            self.next_preload.map(|preload| preload.index),
            || self.random_shuffle_next(),
        )
    }

    pub(super) fn maybe_preload_next(&mut self) {
        if self.next_preload.is_some()
            || self.state.ended_handled
            || !should_preload_next(&self.audio.state())
        {
            return;
        }
        let Some(index) = self.next_queue_index(true) else {
            return;
        };
        let Some(song) = self.state.queue.get(index) else {
            return;
        };
        let Some(api) = self.api.as_ref() else { return };
        // 与播放主路径同一套有效音质规则：预加载流必须与正式播放的
        // 档位/缓存键一致，否则无缝衔接会因音质不符而拒绝复用。
        let quality = TranscodingQuality::effective_for(
            self.config.transcoding_quality,
            song.suffix.as_deref(),
            song.content_type.as_deref(),
        );
        let Ok(url) = api.stream_url(&song.id, quality.max_bit_rate()) else {
            return;
        };
        let cache_key = format!(
            "{}:{}:profile={}",
            api.base_url(),
            song.id,
            quality.cache_profile()
        );
        let duration = song
            .duration
            .and_then(|seconds| u64::try_from(seconds).ok())
            .map(Duration::from_secs);
        self.preload_serial = self.preload_serial.wrapping_add(1);
        self.next_preload = Some(NextPreload {
            id: self.preload_serial,
            index,
            quality,
        });
        let gain = self.song_gain(song);
        self.audio
            .preload(self.preload_serial, url, cache_key, duration, gain);
    }

    pub(super) fn play_next(&mut self, natural_end: bool) -> bool {
        self.freeze_next_preload();
        if let Some(next) = self.next_queue_index(natural_end) {
            if self.state.playback_modes.shuffle
                && self.state.shuffle_history.restore_forward().is_none()
            {
                self.state.shuffle_history.advance(next);
            }
            self.play_queue_index(next);
            true
        } else {
            false
        }
    }

    pub(super) fn advance_queue(&mut self, forward: bool) {
        self.cancel_auto_continue();
        self.freeze_next_preload();
        if forward {
            self.play_next(false);
            return;
        }
        let Some(index) = self.state.queue_index else {
            return;
        };
        let len = self.state.queue.len();
        if len == 0 {
            return;
        }

        if self.state.playback_modes.shuffle {
            if let Some(previous) = self.state.shuffle_history.previous() {
                self.play_queue_index(previous);
            }
            return;
        }

        let wraps = self.state.playback_modes.repeat != RepeatMode::Off;
        let Some(next) = advance_index(index, len, forward, wraps) else {
            return;
        };
        self.play_queue_index(next);
    }

    /// 切换随机开关：取消预加载（下一首选择变化）并持久化。
    pub(super) fn toggle_shuffle(&mut self, cx: &mut Context<Self>) {
        self.cancel_auto_continue();
        self.cancel_next_preload();
        self.state.playback_modes.shuffle = !self.state.playback_modes.shuffle;
        self.config.playback_modes = self.state.playback_modes;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Playback mode save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        self.save_session();
        cx.notify();
    }

    /// 循环模式循环切换：Off → All → One → Off；同样取消预加载并持久化。
    pub(super) fn cycle_repeat(&mut self, cx: &mut Context<Self>) {
        self.cancel_auto_continue();
        self.cancel_next_preload();
        self.state.playback_modes.repeat = self.state.playback_modes.repeat.next();
        self.config.playback_modes = self.state.playback_modes;
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Playback mode save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        self.save_session();
        cx.notify();
    }

    pub fn skip(&mut self, offset: i32) {
        if offset > 0 {
            self.advance_queue(true);
        } else if offset < 0 {
            self.advance_queue(false);
        }
    }

    /// 相对当前进度快进/快退（秒），不越过歌曲起点与终点。
    pub(super) fn seek_by(&mut self, delta_secs: i64) {
        self.cancel_next_preload();
        let playback = self.audio.state();
        let target = seek_target(playback.position, playback.duration, delta_secs);
        if target != playback.position {
            self.seek_playback(target);
        }
    }
}
