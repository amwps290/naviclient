//! Serial snapshots with short read locks, debounce, retry and shutdown flush.

use super::{prune, Store};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const DEBOUNCE: Duration = Duration::from_millis(50);
const RETRY: Duration = Duration::from_secs(5);

struct State {
    store: Store,
    revision: u64,
    saved: u64,
    attempt: u64,
    attempted_revision: u64,
    error: Option<String>,
    notification: Option<String>,
    due: Instant,
    stopped: bool,
    closed: bool,
}

struct Shared {
    state: Mutex<State>,
    ready: Condvar,
    settled: Condvar,
}

pub(super) struct Writer {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}

impl Writer {
    pub(super) fn start(
        store: Store,
        mut save: impl FnMut(&Store) -> anyhow::Result<()> + Send + 'static,
    ) -> std::io::Result<Self> {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                store,
                revision: 0,
                saved: 0,
                attempt: 0,
                attempted_revision: 0,
                error: None,
                notification: None,
                due: Instant::now(),
                stopped: false,
                closed: false,
            }),
            ready: Condvar::new(),
            settled: Condvar::new(),
        });
        let receiver = shared.clone();
        let worker = thread::Builder::new()
            .name("lyrics-cache-writer".into())
            .spawn(move || {
                let _guard = WorkerGuard(receiver.clone());
                loop {
                    let (revision, mut snapshot) = {
                        let mut state = receiver.state.lock().unwrap();
                        loop {
                            if state.stopped {
                                return;
                            }
                            if state.revision > state.saved {
                                let delay = state.due.saturating_duration_since(Instant::now());
                                if delay.is_zero() {
                                    break;
                                }
                                state = receiver.ready.wait_timeout(state, delay).unwrap().0;
                            } else {
                                state = receiver.ready.wait(state).unwrap();
                            }
                        }
                        // Body text is immutable behind Arc; this clones only the
                        // small indices. No body copy, pruning or I/O holds the lock.
                        (state.revision, state.store.clone())
                    };
                    prune(&mut snapshot);
                    let error = save(&snapshot).err().map(|error| format!("{error:#}"));
                    if let Some(error) = &error {
                        log::warn!("lyrics cache save failed: {error}");
                    }
                    let mut snapshot = Some(snapshot);
                    let old_store = {
                        let mut state = receiver.state.lock().unwrap();
                        state.attempt += 1;
                        state.attempted_revision = revision;
                        if error.is_some() && state.error != error {
                            state.notification = error.clone();
                        }
                        state.error = error.clone();
                        let old_store = if error.is_none() {
                            state.saved = revision;
                            state.notification = None;
                            // An older save may never replace newer live choices.
                            (state.revision == revision).then(|| {
                                std::mem::replace(&mut state.store, snapshot.take().unwrap())
                            })
                        } else {
                            if state.revision == revision {
                                state.due = Instant::now() + RETRY;
                            }
                            None
                        };
                        receiver.settled.notify_all();
                        old_store
                    };
                    // Releasing the final body references also happens off-lock.
                    drop(old_store);
                    drop(snapshot);
                }
            })?;
        Ok(Self {
            shared,
            worker: Some(worker),
        })
    }

    pub(super) fn read<T>(&self, read: impl FnOnce(&Store) -> T) -> T {
        read(&self.shared.state.lock().unwrap().store)
    }

    pub(super) fn change(&self, change: impl FnOnce(&mut Store)) -> anyhow::Result<()> {
        let mut state = self.shared.state.lock().unwrap();
        anyhow::ensure!(
            !state.closed && !state.stopped,
            "lyrics cache writer is unavailable"
        );
        change(&mut state.store);
        state.revision = state
            .revision
            .checked_add(1)
            .expect("lyrics cache revision exhausted");
        state.due = Instant::now() + DEBOUNCE;
        self.shared.ready.notify_one();
        Ok(())
    }

    /// Wait for a fresh attempt covering all changes accepted before this call.
    /// A failed attempt returns its error, retaining data for a later retry.
    pub(super) fn flush(&self) -> anyhow::Result<()> {
        let mut state = self.shared.state.lock().unwrap();
        let target = state.revision;
        let baseline = state.attempt;
        state.due = Instant::now();
        self.shared.ready.notify_one();
        loop {
            if state.saved >= target {
                return Ok(());
            }
            if state.attempt > baseline && state.attempted_revision >= target {
                return Err(anyhow::anyhow!(state
                    .error
                    .clone()
                    .unwrap_or_else(|| "lyrics cache save failed".into())));
            }
            anyhow::ensure!(
                !state.closed,
                "lyrics cache writer closed before saving pending changes"
            );
            state = self.shared.settled.wait(state).unwrap();
        }
    }

    pub(super) fn take_error(&self) -> Option<String> {
        self.shared.state.lock().unwrap().notification.take()
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        if let Err(error) = self.flush() {
            log::warn!("pending lyrics cache save failed: {error:#}");
        }
        self.shared.state.lock().unwrap().stopped = true;
        self.shared.ready.notify_one();
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                log::warn!("lyrics cache writer exited unexpectedly");
            }
        }
    }
}

struct WorkerGuard(Arc<Shared>);

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        self.0
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .closed = true;
        self.0.settled.notify_all();
    }
}
