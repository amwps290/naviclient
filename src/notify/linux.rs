use std::path::Path;

use notify_rust::{Notification, Timeout};

use super::worker::{NotificationWorker, TrackNotification};

/// The worker owns native replacement IDs; the UI only submits owned requests.
pub(crate) struct TrackNotifier {
    worker: Option<NotificationWorker>,
}

impl TrackNotifier {
    pub(crate) fn new() -> Self {
        let worker = match NotificationWorker::start(show_notification) {
            Ok(worker) => Some(worker),
            Err(error) => {
                log::debug!("track notification worker failed to start: {error}");
                None
            }
        };
        Self { worker }
    }

    pub(crate) fn track_changed(&mut self, title: &str, body: &str, cover_path: Option<&Path>) {
        if let Some(worker) = &self.worker {
            worker.submit(TrackNotification {
                title: title.to_owned(),
                body: body.to_owned(),
                cover_path: cover_path.map(Path::to_path_buf),
            });
        }
    }

    pub(crate) fn cancel_pending(&self) {
        if let Some(worker) = &self.worker {
            worker.cancel_pending();
        }
    }
}

fn show_notification(request: &TrackNotification, replace_id: Option<u32>) -> anyhow::Result<u32> {
    let notification = build_notification(request, replace_id);
    Ok(notification.show()?.id())
}

fn build_notification(request: &TrackNotification, replace_id: Option<u32>) -> Notification {
    let mut notification = Notification::new();
    notification
        .appname("Navidrome Client")
        .summary(&request.title)
        .body(&request.body)
        .timeout(Timeout::Milliseconds(4000));
    if let Some(id) = replace_id {
        notification.id(id);
    }
    if let Some(path) = request.cover_path.as_deref().filter(|path| path.is_file()) {
        // image-path supplies the large image; icon supports older servers.
        let path = path.to_string_lossy().into_owned();
        notification.icon(&path);
        notification.hint(notify_rust::Hint::ImagePath(path));
    }
    notification
}

#[cfg(test)]
mod tests;
