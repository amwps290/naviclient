//! Coalesced snapshots, retry scheduling and settlement under one private lock.

use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use super::super::server_cache::{CacheKind, CacheSnapshot};
use super::super::server_scope;

const RETRY_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct CacheKey {
    scope: String,
    kind: CacheKind,
}

impl CacheKey {
    pub(super) fn new(server_id: Option<&str>, kind: CacheKind) -> Self {
        Self {
            scope: server_scope(server_id),
            kind,
        }
    }
}

struct PendingWrite {
    revision: u64,
    server_id: Option<String>,
    snapshot: Arc<CacheSnapshot>,
    retry_at: Instant,
}

struct Receipt {
    revision: u64,
    attempt: u64,
    error: Option<String>,
}

#[derive(Default)]
struct State {
    revision: u64,
    attempt: u64,
    pending: BTreeMap<CacheKey, PendingWrite>,
    receipts: BTreeMap<CacheKey, Receipt>,
    in_flight: Option<CacheKey>,
    stopped: bool,
    closed: bool,
}

#[derive(Default)]
pub(super) struct Mailbox {
    state: Mutex<State>,
    ready: Condvar,
    settled: Condvar,
}

/// One immutable save attempt. Completion consumes its key and revision so a
/// superseded attempt cannot settle a newer snapshot.
pub(super) struct WriteTask {
    key: CacheKey,
    revision: u64,
    server_id: Option<String>,
    snapshot: Arc<CacheSnapshot>,
}

impl WriteTask {
    pub(super) fn server_id(&self) -> Option<&str> {
        self.server_id.as_deref()
    }

    pub(super) fn snapshot(&self) -> &CacheSnapshot {
        &self.snapshot
    }
}

impl Mailbox {
    /// At most one waiting snapshot per server and cache kind. The saver owns
    /// retries after acceptance, including empty collections after failed clears.
    pub(super) fn submit(
        &self,
        server_id: Option<&str>,
        snapshot: CacheSnapshot,
    ) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if state.closed || state.stopped {
            return Err("server cache writer is unavailable".into());
        }
        state.revision = state
            .revision
            .checked_add(1)
            .expect("cache revision exhausted");
        let revision = state.revision;
        state.pending.insert(
            CacheKey::new(server_id, snapshot.kind()),
            PendingWrite {
                revision,
                server_id: server_id.map(str::to_string),
                snapshot: Arc::new(snapshot),
                retry_at: Instant::now(),
            },
        );
        self.ready.notify_one();
        Ok(())
    }

    /// Pending immutable data takes precedence over an older disk cache when
    /// switching back after a failed write. Large data is cloned outside the lock.
    pub(super) fn snapshot(
        &self,
        server_id: Option<&str>,
        kind: CacheKind,
    ) -> Option<Arc<CacheSnapshot>> {
        self.state
            .lock()
            .unwrap()
            .pending
            .get(&CacheKey::new(server_id, kind))
            .map(|task| task.snapshot.clone())
    }

    /// Wait for one fresh attempt covering every snapshot accepted before this
    /// call. Failures remain pending, but are returned instead of waiting forever.
    pub(super) fn flush(&self) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        let baseline = state.attempt;
        let targets: Vec<_> = state
            .pending
            .iter_mut()
            .map(|(scope, task)| {
                task.retry_at = Instant::now();
                (scope.clone(), task.revision)
            })
            .collect();
        self.ready.notify_one();
        loop {
            if targets.iter().all(|(scope, revision)| {
                state.receipts.get(scope).is_some_and(|receipt| {
                    receipt.revision >= *revision && receipt.attempt > baseline
                })
            }) {
                let errors = targets
                    .iter()
                    .filter_map(|(scope, _)| state.receipts.get(scope)?.error.as_deref())
                    .collect::<Vec<_>>();
                return if errors.is_empty() {
                    Ok(())
                } else {
                    Err(errors.join("; "))
                };
            }
            if state.closed {
                return Err("cache writer closed before saving pending snapshots".into());
            }
            if targets.iter().any(|(scope, revision)| {
                !state.pending.contains_key(scope)
                    && !state.receipts.get(scope).is_some_and(|receipt| {
                        receipt.revision >= *revision && receipt.attempt > baseline
                    })
            }) {
                return Err("pending snapshot was discarded before saving".into());
            }
            state = self.settled.wait(state).unwrap();
        }
    }

    /// Cancel retries and wait for an existing file replacement before deleting
    /// that server's cache. The UI is the single submitter during this operation.
    pub(super) fn discard(&self, server_id: Option<&str>) {
        let scope = server_scope(server_id);
        let mut state = self.state.lock().unwrap();
        state.pending.retain(|key, _| key.scope != scope);
        self.settled.notify_all();
        while state
            .in_flight
            .as_ref()
            .is_some_and(|key| key.scope == scope)
            && !state.closed
        {
            state = self.settled.wait(state).unwrap();
        }
        state.receipts.retain(|key, _| key.scope != scope);
        self.settled.notify_all();
    }

    pub(super) fn next(&self) -> Option<WriteTask> {
        let mut state = self.state.lock().unwrap();
        loop {
            if state.stopped {
                return None;
            }
            let next = state
                .pending
                .iter()
                .min_by_key(|(_, task)| (task.retry_at, task.revision));
            if let Some((scope, task)) = next {
                let now = Instant::now();
                if task.retry_at <= now {
                    let scope = scope.clone();
                    let task = WriteTask {
                        key: scope.clone(),
                        revision: task.revision,
                        server_id: task.server_id.clone(),
                        snapshot: task.snapshot.clone(),
                    };
                    state.in_flight = Some(scope);
                    return Some(task);
                }
                let timeout = task.retry_at.saturating_duration_since(now);
                state = self.ready.wait_timeout(state, timeout).unwrap().0;
            } else {
                state = self.ready.wait(state).unwrap();
            }
        }
    }

    pub(super) fn complete(&self, task: WriteTask, error: Option<String>) {
        // Keep the attempt's Arc alive until after the lock is released, so
        // removing the last pending copy does not drop a large cache under it.
        let WriteTask {
            key: scope,
            revision,
            snapshot: _snapshot,
            ..
        } = task;
        let mut state = self.state.lock().unwrap();
        state.attempt = state
            .attempt
            .checked_add(1)
            .expect("cache attempt exhausted");
        let attempt = state.attempt;
        if state
            .pending
            .get(&scope)
            .is_some_and(|task| task.revision == revision)
        {
            if error.is_none() {
                state.pending.remove(&scope);
            } else {
                state.pending.get_mut(&scope).unwrap().retry_at = Instant::now() + RETRY_INTERVAL;
            }
        }
        state.receipts.insert(
            scope,
            Receipt {
                revision,
                attempt,
                error,
            },
        );
        state.in_flight = None;
        self.settled.notify_all();
    }

    pub(super) fn worker_guard(self: &Arc<Self>) -> WorkerGuard {
        WorkerGuard(self.clone())
    }

    pub(super) fn stop(&self) {
        self.state.lock().unwrap().stopped = true;
        self.ready.notify_one();
    }

    #[cfg(test)]
    pub(super) fn pending_count(&self) -> usize {
        self.state.lock().unwrap().pending.len()
    }

    #[cfg(test)]
    pub(super) fn retry_at(&self, server_id: Option<&str>, kind: CacheKind) -> Instant {
        self.state.lock().unwrap().pending[&CacheKey::new(server_id, kind)].retry_at
    }

    #[cfg(test)]
    pub(super) fn set_retry_at(&self, server_id: Option<&str>, kind: CacheKind, retry_at: Instant) {
        self.state
            .lock()
            .unwrap()
            .pending
            .get_mut(&CacheKey::new(server_id, kind))
            .unwrap()
            .retry_at = retry_at;
        self.ready.notify_one();
    }
}

pub(super) struct WorkerGuard(Arc<Mailbox>);

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap();
        state.closed = true;
        state.in_flight = None;
        self.0.settled.notify_all();
    }
}

#[cfg(test)]
mod tests;
