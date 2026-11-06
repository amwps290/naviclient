//! A single sender serializes native replacement IDs and coalesces queued work.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TrackNotification {
    pub(super) title: String,
    pub(super) body: String,
    pub(super) cover_path: Option<PathBuf>,
}

#[derive(Default)]
struct Pending {
    notification: Option<TrackNotification>,
    stopped: bool,
}

#[derive(Default)]
struct Mailbox {
    pending: Mutex<Pending>,
    ready: Condvar,
}

impl Mailbox {
    fn next(&self) -> Option<TrackNotification> {
        let mut pending = self.pending.lock().unwrap();
        loop {
            if pending.stopped {
                return None;
            }
            if let Some(notification) = pending.notification.take() {
                return Some(notification);
            }
            pending = self.ready.wait(pending).unwrap();
        }
    }
}

pub(super) struct NotificationWorker {
    mailbox: Arc<Mailbox>,
}

impl NotificationWorker {
    pub(super) fn start(
        mut send: impl FnMut(&TrackNotification, Option<u32>) -> anyhow::Result<u32> + Send + 'static,
    ) -> io::Result<Self> {
        let mailbox = Arc::new(Mailbox::default());
        let receiver = mailbox.clone();
        thread::Builder::new()
            .name("track-notifications".into())
            .spawn(move || {
                let mut last_id = None;
                while let Some(notification) = receiver.next() {
                    last_id = match send(&notification, last_id) {
                        Ok(id) => Some(id),
                        Err(error) => {
                            log::debug!("track notification failed: {error:#}");
                            None
                        }
                    };
                }
            })?;
        Ok(Self { mailbox })
    }

    pub(super) fn submit(&self, notification: TrackNotification) {
        self.mailbox.pending.lock().unwrap().notification = Some(notification);
        self.mailbox.ready.notify_one();
    }

    /// Cancel queued work. A native call that already started may still finish.
    pub(super) fn cancel_pending(&self) {
        self.mailbox.pending.lock().unwrap().notification = None;
    }
}

impl Drop for NotificationWorker {
    fn drop(&mut self) {
        let mut pending = self.mailbox.pending.lock().unwrap();
        pending.notification = None;
        pending.stopped = true;
        self.mailbox.ready.notify_one();
        // Do not join: an in-flight D-Bus call must never delay the UI's exit.
    }
}

#[cfg(test)]
mod tests;
