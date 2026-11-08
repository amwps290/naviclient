//! Foreground preparation ownership, cancellation, and completion delivery.

use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use reqwest::blocking::Client;

use super::decoder::{play_resource, PreparedStream};
use super::error::{FailureError, StreamFailure};
use super::preparation_state::{PreparationKind, PreparationState, PreparationToken};
use super::stream_control::StreamControl;

pub(super) struct StreamRequest {
    pub(super) url: crate::source::HttpResource,
    pub(super) cache_key: String,
    pub(super) cache_dir: PathBuf,
    pub(super) duration: Option<Duration>,
}

pub(super) struct PreparationResult<T = PreparedStream> {
    pub(super) token: PreparationToken,
    pub(super) result: Result<T>,
}

struct PendingTask {
    token: PreparationToken,
    control: Arc<StreamControl>,
    started: bool,
}

pub(super) struct Preparation<T = PreparedStream> {
    state: PreparationState,
    pending: Option<PendingTask>,
    sender: Sender<PreparationResult<T>>,
    receiver: Receiver<PreparationResult<T>>,
}

impl<T> Preparation<T> {
    pub(super) fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            state: PreparationState::default(),
            pending: None,
            sender,
            receiver,
        }
    }

    pub(super) fn state(&self) -> &PreparationState {
        &self.state
    }

    pub(super) fn begin(
        &mut self,
        kind: PreparationKind,
        position: Duration,
        now: Instant,
    ) -> PreparationToken {
        self.cancel_pending();
        let token = self.state.begin(kind, position, now);
        self.pending = Some(PendingTask {
            token,
            control: StreamControl::new(false),
            started: false,
        });
        token
    }

    fn cancel_pending(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.control.cancel();
        }
        // Dropping queued successes releases their playback receivers too.
        while self.receiver.try_recv().is_ok() {}
    }

    pub(super) fn stop(&mut self) {
        self.cancel_pending();
        self.state.stop();
    }

    pub(super) fn complete(&mut self, token: PreparationToken) -> bool {
        if !self.state.complete(token) {
            return false;
        }
        // An accepted stream now belongs to playback. Removing our handle must
        // not cancel its HTTP reader or decoder.
        self.pending = None;
        true
    }

    pub(super) fn deliver(&mut self, token: PreparationToken, result: Result<T>) {
        if self.start(token).is_some() {
            let _ = self.sender.send(PreparationResult { token, result });
        }
    }

    pub(super) fn poll(&mut self) -> Option<PreparationResult<T>> {
        while let Ok(prepared) = self.receiver.try_recv() {
            if self.complete(prepared.token) {
                return Some(prepared);
            }
            log::debug!(
                "discarding stale audio preparation; generation={} current_generation={} kind={:?}",
                prepared.token.generation,
                self.state.generation(),
                prepared.token.kind
            );
        }
        None
    }

    fn start(&mut self, token: PreparationToken) -> Option<Arc<StreamControl>> {
        let pending = self
            .pending
            .as_mut()
            .filter(|pending| pending.token == token && !pending.started)?;
        pending.started = true;
        Some(pending.control.clone())
    }

    pub(super) fn preparing(&self) -> bool {
        self.state.preparing()
    }

    pub(super) fn paused(&self) -> bool {
        self.state.paused()
    }

    pub(super) fn set_paused(&mut self, paused: bool) {
        self.state.set_paused(paused);
    }
}

impl<T: Send + 'static> Preparation<T> {
    fn spawn_with(
        &mut self,
        token: PreparationToken,
        prepare: impl FnOnce(Arc<StreamControl>) -> Result<T> + Send + 'static,
        spawn: impl FnOnce(Box<dyn FnOnce() + Send>) -> io::Result<()>,
    ) {
        if let Some(control) = self.start(token) {
            spawn_with(self.sender.clone(), token, control, prepare, spawn);
        }
    }
}

impl Preparation {
    pub(super) fn spawn(
        &mut self,
        token: PreparationToken,
        client: Client,
        request: StreamRequest,
    ) {
        self.spawn_with(
            token,
            move |control| {
                play_resource(
                    client,
                    request.url,
                    &request.cache_dir,
                    &request.cache_key,
                    request.duration,
                    token.position,
                    control,
                )
            },
            |task| {
                thread::Builder::new()
                    .name("audio-prepare".to_string())
                    .spawn(task)
                    .map(|_| ())
            },
        );
    }
}

impl<T> Drop for Preparation<T> {
    fn drop(&mut self) {
        // Blocking reads finish under the client's I/O timeout. Never join them
        // here or put the next foreground request behind a cancelled read.
        self.cancel_pending();
    }
}

fn spawn_with<T: Send + 'static>(
    tx: Sender<PreparationResult<T>>,
    token: PreparationToken,
    control: Arc<StreamControl>,
    prepare: impl FnOnce(Arc<StreamControl>) -> Result<T> + Send + 'static,
    spawn: impl FnOnce(Box<dyn FnOnce() + Send>) -> io::Result<()>,
) {
    let failure_tx = tx.clone();
    let task = Box::new(move || {
        if control.check().is_err() {
            return;
        }
        let result = prepare(control.clone());
        // Startup failures can themselves cancel the control. Deliver current
        // errors regardless; only discard cancelled successes here.
        if result.is_ok() && control.check().is_err() {
            return;
        }
        let _ = tx.send(PreparationResult { token, result });
    });
    if let Err(error) = spawn(task) {
        // Report through the same completion path as network/decode failures,
        // so the player leaves buffering and exposes a usable error to the UI.
        let _ = failure_tx.send(PreparationResult {
            token,
            result: Err(anyhow!(FailureError::new(
                StreamFailure::Preparation,
                format!("failed to start the audio preparation thread: {error}"),
            ))),
        });
    }
}

#[cfg(test)]
mod tests;
