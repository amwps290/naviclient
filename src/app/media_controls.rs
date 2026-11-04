//! 播放重试、媒体会话、全局快捷键轮询和外部媒体命令。

use super::*;

impl NavidromeApp {
    /// 手动重试当前由：从出错位置（有位置时）重新拉流，并重置自动重连计数。
    pub(crate) fn retry_current_track(&mut self) {
        let Some(index) = self.state.queue_index else {
            return;
        };
        let playback = self.audio.state();
        let position = if playback.error.is_some() {
            self.resume_position.unwrap_or(playback.position)
        } else {
            playback.position
        };
        log::info!(
            "manual playback retry requested; index={index} position_ms={}",
            position.as_millis()
        );
        self.reconnect = None;
        self.reconnect_attempts = None;
        self.state.ended_handled = false;
        self.play_queue_index_at(index, position);
    }

    pub fn toggle_playback(&mut self) {
        self.cancel_auto_continue();
        self.sync_gapless_transition();
        let playback = self.audio.state();
        if playback.active {
            if playback.paused {
                self.audio.resume()
            } else {
                self.audio.pause()
            }
        } else if let Some(index) = self.state.queue_index {
            let position = self.resume_position.take().unwrap_or_default();
            self.play_queue_index_at(index, position);
        }
    }

    #[allow(dead_code)]
    pub fn stop_playback(&mut self) {
        self.cancel_auto_continue();
        self.cancel_next_preload();
        self.audio.stop();
        self.resume_position = None;
        self.state.now_playing = None;
        self.state.now_playing_quality = None;
        self.state.lyrics = None;
        self.state.lyrics_song_id = None;
        self.state.lyrics_loading = false;
        self.state.lyrics_error = None;
        self.state.ended_handled = true;
    }

    pub(super) fn media_capabilities(&self) -> (bool, bool, bool) {
        let Some(index) = self
            .state
            .queue_index
            .filter(|i| *i < self.state.queue.len())
        else {
            return (false, false, false);
        };
        let can_play = self.api.is_some();
        let wraps = self.state.playback_modes.repeat != RepeatMode::Off;
        let (next, previous) = if self.state.playback_modes.shuffle {
            (true, self.state.shuffle_history.played.len() > 1)
        } else {
            (
                advance_index(index, self.state.queue.len(), true, wraps).is_some(),
                advance_index(index, self.state.queue.len(), false, wraps).is_some(),
            )
        };
        (can_play, can_play && next, can_play && previous)
    }

    pub(super) fn sync_media_session(&mut self, playback: &crate::audio::PlaybackState) {
        let (can_play, can_next, can_previous) = self.media_capabilities();
        let song = self.state.now_playing.as_ref();
        let cover = song
            .and_then(|song| song.cover_art.as_ref())
            .and_then(|id| self.state.covers.get(id));
        self.media_session.update(MediaUpdate {
            server: self.api.as_ref().map_or("", |api| api.base_url()),
            song,
            cover,
            playback,
            position: self.resume_position.unwrap_or(playback.position),
            volume: effective_volume(self.config.volume, self.muted),
            can_play,
            can_next,
            can_previous,
        });
    }

    pub(super) fn media_seek(&mut self, position: Duration) {
        if self.audio.state().active {
            self.seek_playback(position);
        } else if self.state.now_playing.is_some() {
            // Seeking a restored, paused session must not start audio playback.
            self.resume_position = Some(position);
            self.media_session.restored_position_changed();
        }
    }

    pub(super) fn poll_global_shortcuts(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut changed = false;
        for _ in 0..32 {
            let Some(event) = self.global_shortcuts.try_recv() else {
                break;
            };
            changed = true;
            match event {
                GlobalShortcutEvent::Triggered(action) => {
                    self.execute_playback_shortcut(action, window, cx);
                }
                GlobalShortcutEvent::RegistrationReport { failures } => {
                    self.state.global_shortcut_failures = failures.into_iter().collect();
                }
            }
        }
        changed
    }

    pub(super) fn poll_media_commands(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut changed = false;
        // Maintain intent across a batch, even before the audio worker consumes
        // it. Repeated Play/Pause commands are idempotent, unlike PlayPause.
        let playback = self.audio.state();
        let mut active = playback.active;
        let mut paused = playback.paused;
        for _ in 0..32 {
            let Some(command) = self.media_session.next_command() else {
                break;
            };
            changed = true;
            match command {
                MediaCommand::Play | MediaCommand::Pause | MediaCommand::PlayPause => {
                    let play = match command {
                        MediaCommand::Play => true,
                        MediaCommand::Pause => false,
                        _ => !active || paused,
                    };
                    if play && self.media_capabilities().0 {
                        if active {
                            if paused {
                                self.audio.resume();
                            }
                        } else if let Some(index) = self.state.queue_index {
                            let position = self.resume_position.take().unwrap_or_default();
                            self.play_queue_index_at(index, position);
                            active = true;
                        }
                        paused = false;
                    } else if !play && active && !paused {
                        self.audio.pause();
                        paused = true;
                    }
                }
                MediaCommand::Stop => {
                    self.stop_playback();
                    active = false;
                    paused = false;
                }
                MediaCommand::Next | MediaCommand::Previous => {
                    let next = matches!(command, MediaCommand::Next);
                    let (_, can_next, can_previous) = self.media_capabilities();
                    if (next && can_next) || (!next && can_previous) {
                        self.skip(if next { 1 } else { -1 });
                        active = true;
                        paused = false;
                    }
                }
                MediaCommand::Seek(offset) => {
                    self.cancel_next_preload();
                    if self.state.now_playing.is_some() {
                        let playback = self.audio.state();
                        let position = self.resume_position.unwrap_or(playback.position);
                        let duration = playback.duration.or_else(|| {
                            self.state
                                .now_playing
                                .as_ref()
                                .and_then(|song| song.duration)
                                .map(|v| Duration::from_secs(v.max(0) as u64))
                        });
                        if offset > 0
                            && duration.is_some_and(|limit| {
                                position.saturating_add(Duration::from_micros(offset as u64))
                                    > limit
                            })
                        {
                            self.skip(1);
                        } else {
                            self.media_seek(media_session::seek_offset(position, offset, duration));
                        }
                    }
                }
                MediaCommand::SetPosition { track_id, position } => {
                    self.cancel_next_preload();
                    if let Some(song) = self.state.now_playing.as_ref() {
                        let server = self.api.as_ref().map_or("", |api| api.base_url());
                        let duration = self.audio.state().duration.or_else(|| {
                            song.duration.map(|v| Duration::from_secs(v.max(0) as u64))
                        });
                        if media_session::track_id(server, &song.id) == track_id
                            && duration.is_none_or(|limit| position <= limit)
                        {
                            self.media_seek(position);
                        }
                    }
                }
                MediaCommand::Volume(volume) => {
                    self.volume_slider.update(cx, |slider, cx| {
                        slider.set_value(volume * 100.0, window, cx)
                    });
                    self.set_volume(volume, cx);
                }
                MediaCommand::Raise => window.activate_window(),
                MediaCommand::Quit => {
                    self.save_session();
                    self.quitting.store(true, Ordering::Relaxed);
                    cx.quit();
                    break;
                }
            }
        }
        changed
    }
}
