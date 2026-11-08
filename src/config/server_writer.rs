//! Serial, coalesced server-cache writes with retry ownership and explicit flushes.

use std::io;
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use crate::models::{Genre, ListeningHistory, PlaybackSession};
use crate::recommendation::DailyMixSnapshot;
use crate::source::SourceId;

use super::server_cache::{save_snapshot, CacheKind, CacheSnapshot};

mod mailbox;

use mailbox::Mailbox;

pub(crate) struct ServerCacheWriter {
    mailbox: Arc<Mailbox>,
    worker: Option<JoinHandle<()>>,
}

impl ServerCacheWriter {
    pub(crate) fn start() -> io::Result<Self> {
        Self::start_with(save_snapshot)
    }

    fn start_with(
        mut save: impl FnMut(Option<&str>, &CacheSnapshot) -> anyhow::Result<()> + Send + 'static,
    ) -> io::Result<Self> {
        let mailbox = Arc::new(Mailbox::default());
        let receiver = mailbox.clone();
        let worker = thread::Builder::new()
            .name("server-cache-writer".into())
            .spawn(move || {
                let _guard = receiver.worker_guard();
                while let Some(task) = receiver.next() {
                    let error = save(task.server_id(), task.snapshot())
                        .err()
                        .map(|error| format!("{error:#}"));
                    if let Some(error) = &error {
                        log::warn!("failed to save {}: {error}", task.snapshot().kind().label());
                    }
                    receiver.complete(task, error);
                }
            })?;
        Ok(Self {
            mailbox,
            worker: Some(worker),
        })
    }

    /// At most one waiting snapshot per server and cache kind. The saver owns
    /// retries after acceptance, including empty collections after failed clears.
    pub(crate) fn submit(
        &self,
        server_id: Option<&str>,
        snapshot: impl Into<CacheSnapshot>,
    ) -> Result<(), String> {
        self.mailbox.submit(server_id, snapshot.into())
    }

    pub(crate) fn submit_source(
        &self,
        source: &SourceId,
        snapshot: impl Into<CacheSnapshot>,
    ) -> Result<(), String> {
        self.mailbox.submit(
            Some(&source.profile_id),
            CacheSnapshot::scoped(source.clone(), snapshot),
        )
    }

    pub(crate) fn pending_source_session(&self, source: &SourceId) -> Option<PlaybackSession> {
        match self
            .mailbox
            .source_snapshot(source, CacheKind::Session)?
            .value()
        {
            CacheSnapshot::Session(value) => Some(value.clone()),
            _ => None,
        }
    }

    pub(crate) fn pending_source_history(&self, source: &SourceId) -> Option<ListeningHistory> {
        match self
            .mailbox
            .source_snapshot(source, CacheKind::ListeningHistory)?
            .value()
        {
            CacheSnapshot::ListeningHistory(value) => Some(value.clone()),
            _ => None,
        }
    }

    pub(crate) fn pending_source_loudness(
        &self,
        source: &SourceId,
    ) -> Option<std::collections::HashMap<String, (f32, f32)>> {
        match self
            .mailbox
            .source_snapshot(source, CacheKind::Loudness)?
            .value()
        {
            CacheSnapshot::Loudness(value) => Some(value.clone()),
            _ => None,
        }
    }

    pub(crate) fn discard_source(&self, source: &SourceId) {
        self.mailbox.discard_source(source);
    }

    #[cfg(test)]
    pub(crate) fn pending_session(&self, server_id: Option<&str>) -> Option<PlaybackSession> {
        match self
            .mailbox
            .snapshot(server_id, CacheKind::Session)?
            .value()
        {
            CacheSnapshot::Session(value) => Some(value.clone()),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn pending_loudness(
        &self,
        server_id: Option<&str>,
    ) -> Option<std::collections::HashMap<String, (f32, f32)>> {
        match self
            .mailbox
            .snapshot(server_id, CacheKind::Loudness)?
            .value()
        {
            CacheSnapshot::Loudness(value) => Some(value.clone()),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn pending_history(&self, server_id: Option<&str>) -> Option<ListeningHistory> {
        match self
            .mailbox
            .snapshot(server_id, CacheKind::ListeningHistory)?
            .value()
        {
            CacheSnapshot::ListeningHistory(value) => Some(value.clone()),
            _ => None,
        }
    }

    pub(crate) fn pending_genres(&self, server_id: Option<&str>) -> Option<Vec<Genre>> {
        match self.mailbox.snapshot(server_id, CacheKind::Genres)?.value() {
            CacheSnapshot::Genres(value) => Some(value.clone()),
            _ => None,
        }
    }

    pub(crate) fn pending_daily_mix(&self, server_id: Option<&str>) -> Option<DailyMixSnapshot> {
        match self
            .mailbox
            .snapshot(server_id, CacheKind::DailyMix)?
            .value()
        {
            CacheSnapshot::DailyMix(value) => Some(value.clone()),
            _ => None,
        }
    }

    /// Wait for one fresh attempt covering every snapshot accepted before this
    /// call. Failures remain pending, but are returned instead of waiting forever.
    pub(crate) fn flush(&self) -> Result<(), String> {
        self.mailbox.flush()
    }

    /// Cancel retries and wait for an existing file replacement before deleting
    /// that server's cache. The UI is the single submitter during this operation.
    #[cfg(test)]
    pub(crate) fn discard(&self, server_id: Option<&str>) {
        self.mailbox.discard(server_id);
    }
}

impl Drop for ServerCacheWriter {
    fn drop(&mut self) {
        let _ = self.flush();
        self.mailbox.stop();
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                log::warn!("server cache writer exited unexpectedly");
            }
        }
    }
}

#[cfg(test)]
mod tests;
