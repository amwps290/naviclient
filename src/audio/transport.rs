//! Pending transport intent and ordered acknowledgement by the audio worker.

use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::worker::Command;
use super::{set_state, PlaybackState};

#[derive(Debug)]
pub(super) struct WorkerCommand {
    pub(super) command: Command,
    pub(super) serial: Option<u64>,
}

impl From<Command> for WorkerCommand {
    fn from(command: Command) -> Self {
        Self {
            command,
            serial: None,
        }
    }
}

struct PendingIntent {
    serial: u64,
    playback: PlaybackState,
    replaces_output: bool,
}

#[derive(Default)]
pub(super) struct Transport {
    serial: u64,
    pending: Option<PendingIntent>,
}

impl Transport {
    #[cfg(test)]
    pub(super) fn pending(&self) -> bool {
        self.pending.is_some()
    }

    pub(super) fn observe(&mut self, observed: PlaybackState) -> PlaybackState {
        if observed.worker_closed
            || self
                .pending
                .as_ref()
                .is_some_and(|pending| pending.serial == observed.command_serial)
        {
            self.pending = None;
        }
        let Some(pending) = self.pending.as_ref() else {
            return observed;
        };
        let mut playback = pending.playback.clone();
        // Device changes and completed seeks are observed independently of a
        // pending user command. Do not freeze these worker-owned diagnostics.
        playback.output_name = observed.output_name;
        playback.output_sample_rate = observed.output_sample_rate;
        playback.output_channels = observed.output_channels;
        playback.seek_serial = observed.seek_serial;
        playback.command_serial = observed.command_serial;
        playback
    }

    pub(super) fn send(
        &mut self,
        sender: &Sender<WorkerCommand>,
        observed: PlaybackState,
        command: Command,
    ) -> std::result::Result<(), mpsc::SendError<WorkerCommand>> {
        let current = self.observe(observed);
        let projected = project(&current, &command);
        let replaces_output = matches!(
            command,
            Command::Play { .. } | Command::Seek(_) | Command::Stop
        ) || self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.replaces_output);
        let serial = projected.as_ref().map(|_| {
            self.serial = self.serial.wrapping_add(1).max(1);
            self.serial
        });
        // The enclosing handle lock keeps sequence assignment, delivery and
        // projected state in the same order across cloned handles.
        if let Err(error) = sender.send(WorkerCommand { command, serial }) {
            self.pending = None;
            return Err(error);
        }
        if let Some(playback) = projected {
            self.pending = Some(PendingIntent {
                serial: serial.unwrap(),
                playback,
                replaces_output,
            });
        }
        Ok(())
    }

    pub(super) fn progress(&mut self, mut observed: PlaybackState) -> PlaybackState {
        let projected = self.observe(observed.clone());
        if let Some(pending) = self.pending.as_ref() {
            if pending.replaces_output {
                // Keep the requested position as an inactive baseline: a short
                // seek must not be counted as listening when output resumes.
                observed = projected;
                observed.output_connected = false;
            } else {
                // Pause immediately excludes progress; resume waits for the
                // worker so an old paused position cannot count as listening.
                observed.paused |= pending.playback.paused;
            }
        }
        observed.active &= observed.output_connected && !observed.worker_closed;
        observed
    }
}

pub(super) struct WorkerGuard(pub(super) Arc<Mutex<PlaybackState>>);

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        set_state(&self.0, |state| {
            state.worker_closed = true;
            state.active = false;
            state.output_connected = false;
            state.ended = false;
        });
    }
}

fn project(current: &PlaybackState, command: &Command) -> Option<PlaybackState> {
    let mut playback = current.clone();
    match command {
        Command::Play {
            start,
            duration_hint,
            ..
        } => {
            reset_track(&mut playback);
            playback.active = true;
            playback.paused = start.paused;
            playback.position =
                duration_hint.map_or(start.position, |limit| start.position.min(limit));
            playback.duration = *duration_hint;
        }
        Command::Seek(position) => {
            playback.output_connected = false;
            playback.position = playback
                .duration
                .map_or(*position, |limit| (*position).min(limit));
            playback.buffered = Duration::ZERO;
            playback.fully_buffered = false;
            playback.ended = false;
            playback.error = None;
            playback.failure = None;
            playback.stall = None;
        }
        Command::Pause => playback.paused = true,
        Command::Resume => playback.paused = false,
        Command::Stop => reset_track(&mut playback),
        _ => return None,
    }
    Some(playback)
}

fn reset_track(playback: &mut PlaybackState) {
    *playback = PlaybackState {
        seek_serial: playback.seek_serial,
        command_serial: playback.command_serial,
        output_name: playback.output_name.take(),
        output_sample_rate: playback.output_sample_rate,
        output_channels: playback.output_channels,
        ..Default::default()
    };
}

#[cfg(test)]
mod tests;
