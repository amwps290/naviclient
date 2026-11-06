//! 队列播放推进、Scrobble、响度补偿、预加载和无缝切歌控制。

use super::logic::should_scrobble;
use super::occurrence::Selection;
use super::start::{select_state, PreparedStart, StartError, StartTarget};
use crate::app::prelude::*;
use crate::app::queue::logic::ShuffleHistory;
use crate::app::queue::navigation::QueueStep;
use rand::thread_rng;

impl NavidromeApp {
    pub(in crate::app) fn play_queue_index(&mut self, index: usize) -> bool {
        let Some(target) = StartTarget::existing(&self.state.queue, index) else {
            return false;
        };
        let Some(start) = self.prepare_playback_start(target, Duration::ZERO, Selection::New)
        else {
            return false;
        };
        self.activate_playback_start(start)
    }

    pub(in crate::app) fn play_queue_index_with_history(
        &mut self,
        index: usize,
        history: ShuffleHistory,
    ) -> bool {
        let Some(target) = StartTarget::existing(&self.state.queue, index) else {
            return false;
        };
        let Some(start) = self.prepare_playback_start(target, Duration::ZERO, Selection::New)
        else {
            return false;
        };
        self.activate_playback_start(start.with_history(history))
    }

    pub(in crate::app) fn play_queue_index_at(&mut self, index: usize, position: Duration) -> bool {
        let Some(target) = StartTarget::existing(&self.state.queue, index) else {
            return false;
        };
        let Some(start) = self.prepare_playback_start(target, position, Selection::Resume) else {
            return false;
        };
        self.activate_playback_start(start)
    }

    pub(in crate::app) fn prepare_playback_start(
        &mut self,
        target: StartTarget,
        position: Duration,
        selection: Selection,
    ) -> Option<PreparedStart> {
        match PreparedStart::prepare(
            target,
            self.api.as_ref(),
            self.config.transcoding_quality,
            position,
            selection,
        ) {
            Ok(start) => Some(start),
            Err(error) => {
                self.report_start_error(error);
                None
            }
        }
    }

    pub(in crate::app) fn activate_playback_start(&mut self, mut start: PreparedStart) -> bool {
        self.freeze_next_preload();
        start.request.gain = self.song_gain(start.song());
        let preload_id =
            self.preload
                .reusable_id(start.entry(), start.request.quality, start.request.position);
        let audio = self.audio.clone();
        let new_occurrence = start.new_occurrence();
        let effects = match start.commit(&mut self.state, &mut self.session, |request| {
            audio.play(
                request.url.clone(),
                request.cache_key.clone(),
                request.duration,
                request.gain,
                request.position,
                preload_id,
            )
        }) {
            Ok(effects) => effects,
            Err(error) => {
                self.report_start_error(error);
                return false;
            }
        };
        if new_occurrence {
            self.reconnect.cancel();
        }
        self.preload.clear();
        if effects.finished_listening {
            self.persist_listening_history();
        }
        self.ensure_cover(effects.song.cover_art.as_deref(), true);
        self.load_lyrics(&effects.song);
        self.report_now_playing(&effects.song, effects.position);
        log::info!(
            "queue playback started; song_id={} quality={} position_ms={}",
            effects.song.id,
            self.state.now_playing_quality.unwrap().label(),
            effects.position.as_millis()
        );
        true
    }

    fn report_start_error(&mut self, error: StartError) {
        match error {
            StartError::NoServer => self.notify_error(t!("Configure a server before playing")),
            StartError::Request(error) => self.notify_error(t!(
                "Stream request failed: %{error}",
                error = format!("{error:#}")
            )),
            StartError::OutputUnavailable => self.notify_error(t!(
                "could not start playback: %{error}",
                error = StreamFailure::Preparation.cause()
            )),
            StartError::Stale => {}
        }
    }

    /// Report once per playback occurrence; retries retain the reporting state.
    pub(in crate::app) fn report_now_playing(&mut self, song: &Song, position: Duration) {
        let Some(api) = self.api.clone() else { return };
        if !self.state.occurrence.claim_report(&song.id) {
            return;
        }
        let song_id = song.id.clone();
        let time_secs = u32::try_from(position.as_secs()).unwrap_or(u32::MAX);
        self.spawn_future(async move {
            if let Err(error) = api.update_now_playing(&song_id, time_secs).await {
                log::warn!("updateNowPlaying failed (non-fatal): {error:#}");
            }
        });
    }

    /// 当前歌曲播放达到有效阈值后，向 Navidrome 发送一次 Scrobble（每首一次）。
    pub(in crate::app) fn maybe_scrobble_current(&mut self) {
        let Some(song_id) = self.state.now_playing.as_ref().map(|song| song.id.clone()) else {
            return;
        };
        let playback = self.audio.progress();
        if !should_scrobble(&playback) {
            return;
        }
        let Some(api) = self.api.clone() else { return };
        if !self.state.occurrence.claim_scrobble(&song_id) {
            return;
        }
        self.spawn_future(async move {
            if let Err(error) = api.scrobble(&song_id, true).await {
                log::warn!("scrobble failed (non-fatal): {error:#}");
            }
        });
    }

    /// 自然播放结束时，若尚未提交 Scrobble 则提交（完整播放一定有效）。
    pub(in crate::app) fn ensure_scrobble_on_end(&mut self) {
        let Some(song) = self.state.now_playing.clone() else {
            return;
        };
        let Some(api) = self.api.clone() else { return };
        if !self.state.occurrence.claim_scrobble(&song.id) {
            return;
        }
        let song_id = song.id.clone();
        self.spawn_future(async move {
            if let Err(error) = api.scrobble(&song_id, true).await {
                log::warn!("scrobble on end failed (non-fatal): {error:#}");
            }
        });
    }

    pub(in crate::app) fn select_playing(
        &mut self,
        index: usize,
        song: &Song,
        quality: TranscodingQuality,
        selection: Selection,
    ) {
        if select_state(&mut self.state, index, song, quality, selection) {
            self.persist_listening_history();
        }
        self.session.clear_position();
        self.ensure_cover(song.cover_art.as_deref(), true);
        self.load_lyrics(song);
    }

    pub(in crate::app) fn seek_playback(&mut self, position: Duration) {
        self.reconnect.cancel();
        self.cancel_next_preload();
        let playback = self.audio.state();
        let Some(timeline) = self
            .session
            .timeline(&playback, self.state.now_playing.as_ref())
        else {
            return;
        };
        let timeline = timeline.with_position(position);
        if playback.active {
            self.audio.seek(timeline.position);
        } else {
            self.session.set_position(timeline);
            self.media_session.restored_position_changed();
        }
    }

    pub(in crate::app) fn next_queue_index(&self, natural_end: bool) -> Option<usize> {
        self.state.queue_navigation().next_index(
            natural_end,
            self.preload.reserved_index(&self.state.queue),
            &mut thread_rng(),
        )
    }

    pub(in crate::app) fn play_next(&mut self, natural_end: bool) -> bool {
        self.freeze_next_preload();
        let Some(step) = self.state.queue_navigation().next_playback(
            natural_end,
            self.preload.reserved_index(&self.state.queue),
            &mut thread_rng(),
        ) else {
            return false;
        };
        self.apply_queue_step(step)
    }

    pub(in crate::app) fn advance_queue(&mut self, forward: bool) {
        self.state.cancel_deferred_playback();
        self.freeze_next_preload();
        if forward {
            self.play_next(false);
            return;
        }
        if let Some(step) = self.state.queue_navigation().previous_playback() {
            self.apply_queue_step(step);
        }
    }

    fn apply_queue_step(&mut self, step: QueueStep) -> bool {
        match step {
            QueueStep::KeepHistory(index) => self.play_queue_index(index),
            QueueStep::WithHistory { index, history } => {
                self.play_queue_index_with_history(index, history)
            }
        }
    }

    pub fn skip(&mut self, offset: i32) {
        if offset > 0 {
            self.advance_queue(true);
        } else if offset < 0 {
            self.advance_queue(false);
        }
    }

    /// 相对当前进度快进/快退（秒），不越过歌曲起点与终点。
    pub(in crate::app) fn seek_by(&mut self, delta_secs: i64) {
        self.cancel_next_preload();
        let Some(timeline) = self.playback_timeline() else {
            return;
        };
        let target = timeline.seek_seconds(delta_secs);
        if target != timeline.position {
            self.seek_playback(target);
        }
    }
}
