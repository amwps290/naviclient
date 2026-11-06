//! A real command channel and pending transport state, without an audio worker.

use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use super::transport::{self, Transport, WorkerCommand};
use super::worker::Command;
use super::{AudioHandle, PlaybackState};

pub(crate) struct Fixture {
    pub(crate) audio: AudioHandle,
    receiver: Option<Receiver<WorkerCommand>>,
}

impl Fixture {
    pub(crate) fn new(playback: PlaybackState) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            audio: AudioHandle {
                tx,
                state: Arc::new(Mutex::new(playback)),
                transition: Arc::new(Mutex::new(None)),
                transport: Arc::new(Mutex::new(Transport::default())),
            },
            receiver: Some(rx),
        }
    }

    pub(crate) fn disconnect(&mut self) {
        self.receiver = None;
    }

    pub(crate) fn publish_next(&self, mut playback: PlaybackState) {
        let command = self.receiver.as_ref().unwrap().try_recv().unwrap();
        playback.command_serial = command.serial.unwrap();
        *self.audio.state.lock().unwrap() = playback;
    }

    pub(crate) fn publish(&self, mut playback: PlaybackState) {
        let mut state = self.audio.state.lock().unwrap();
        playback.command_serial = state.command_serial;
        *state = playback;
    }

    pub(crate) fn exit_with_error(&mut self, error: &str) {
        self.audio.state.lock().unwrap().error = Some(error.into());
        drop(transport::WorkerGuard(self.audio.state.clone()));
        self.disconnect();
    }

    pub(crate) fn commands(&self) -> Vec<&'static str> {
        self.receiver
            .as_ref()
            .unwrap()
            .try_iter()
            .map(|message| match message.command {
                Command::Play { .. } => "play",
                Command::Pause => "pause",
                Command::Resume => "resume",
                Command::Stop => "stop",
                Command::Seek(_) => "seek",
                _ => panic!("unexpected non-transport command"),
            })
            .collect()
    }
}
