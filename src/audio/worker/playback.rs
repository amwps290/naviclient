//! Prepared-stream output attachment, preloading eligibility, and playback publication.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rodio::Player;
use rust_i18n::t;

use super::{PlaybackWorker, ReadyOutput};
use crate::audio::error::{classify_anyhow, FailureError};
use crate::audio::gapless::GaplessSource;
use crate::audio::output_track::OutputTrack;
use crate::audio::preparation_state::{PreparationKind, PreparationState};
use crate::audio::track_state::{OutputSnapshot, TrackState};
use crate::audio::{set_state, PlaybackState};

pub(in crate::audio) fn apply_playback_intent(player: &Player, paused: bool) {
    if paused {
        player.pause();
    } else {
        player.play();
    }
}

fn output_snapshot(player: &Player) -> OutputSnapshot {
    OutputSnapshot {
        position: player.get_pos(),
        paused: player.is_paused(),
        empty: player.empty(),
    }
}

pub(super) fn publish_track(
    state: &Arc<Mutex<PlaybackState>>,
    current: &TrackState,
    preparation: &PreparationState,
    player: &Player,
    seek_completed: bool,
) {
    let snapshot = current.snapshot(output_snapshot(player), preparation, Instant::now());
    set_state(state, |s| {
        snapshot.apply(s);
        if seek_completed {
            s.seek_serial = s.seek_serial.wrapping_add(1);
        }
    });
}

impl PlaybackWorker {
    pub(super) fn poll_preparation(&mut self, ready_output: &mut Option<ReadyOutput>) {
        while let Some(prepared) = self.preparation.poll() {
            match prepared.result {
                Ok(stream) => {
                    *ready_output = Some((
                        prepared.token,
                        OutputTrack::new(
                            stream,
                            self.current.gain(),
                            self.output.device.channels,
                            self.output.device.sample_rate,
                            self.output.eq.clone(),
                        ),
                    ));
                }
                Err(error) => {
                    let kind = classify_anyhow(&error);
                    let detail = match prepared.token.kind {
                        PreparationKind::Play => t!(
                            "could not start playback: %{error}",
                            error = format!("{error:#}")
                        )
                        .to_string(),
                        PreparationKind::Seek => {
                            t!("could not seek: %{error}", error = format!("{error:#}")).to_string()
                        }
                    };
                    log::warn!(
                        "audio preparation failed; kind={:?}: {error:#}",
                        prepared.token.kind
                    );
                    let failure = FailureError::new(kind, detail);
                    self.current.record_error(failure);
                    publish_track(
                        &self.state,
                        &self.current,
                        self.preparation.state(),
                        &self.output.device.player,
                        false,
                    );
                }
            }
        }
    }

    pub(super) fn attach_output(&mut self, ready: ReadyOutput) {
        let (token, track) = ready;
        let kind = token.kind;
        let start_position = token.position;
        self.current.accept_ready(track.info.clone());
        self.output.device.player.set_volume(self.volume);
        apply_playback_intent(&self.output.device.player, self.preparation.paused());
        self.output.device.player.append(GaplessSource::new(
            track,
            self.output.handoff.clone(),
            self.output.device.channels,
            self.output.device.sample_rate,
        ));
        log::info!(
            "playback prepared; kind={kind:?} start_ms={}",
            start_position.as_millis()
        );
        publish_track(
            &self.state,
            &self.current,
            self.preparation.state(),
            &self.output.device.player,
            matches!(kind, PreparationKind::Seek),
        );
    }

    pub(super) fn poll_next_and_publish(&mut self) {
        let position = self.current.position(self.output.device.player.get_pos());
        // A UI request may arrive while a seek is still preparing. Keep its
        // reservation, but defer network I/O until the current audio is safe.
        let can_preload = !self.preparation.preparing()
            && !self.output.device.player.empty()
            && !self.preparation.paused()
            && self.current.fully_buffered()
            && self.current.duration().is_some_and(|duration| {
                let remaining = duration.saturating_sub(position);
                !remaining.is_zero() && remaining <= Duration::from_secs(15)
            });
        self.next_track.poll(
            can_preload,
            &self.cache_dir,
            self.output.device.channels,
            self.output.device.sample_rate,
            &self.output.eq,
        );
        let device_snapshot = output_snapshot(&self.output.device.player);
        let snapshot =
            self.current
                .snapshot(device_snapshot, self.preparation.state(), Instant::now());
        if self
            .current
            .take_terminal_log(device_snapshot.empty, self.preparation.preparing())
        {
            if let Some(error) = snapshot.playback.error.as_deref() {
                log::error!("playback stopped with error: {error}");
            } else {
                log::info!("playback reached end; position_ms={}", position.as_millis());
            }
        }
        set_state(&self.state, |s| {
            snapshot.apply(s);
            s.command_serial = self.command_serial;
        });
    }
}
