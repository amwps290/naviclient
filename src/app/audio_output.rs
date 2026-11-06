//! Output device discovery state and playback device selection.

use super::library_requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use super::*;
use crate::audio::OutputDevice;

#[derive(Default)]
pub(super) struct OutputDevices {
    items: Vec<OutputDevice>,
    load: LibraryLoad,
}

impl OutputDevices {
    pub(super) fn items(&self) -> &[OutputDevice] {
        &self.items
    }

    pub(super) fn loading(&self) -> bool {
        self.load.loading()
    }

    pub(super) fn begin(&mut self, requests: &mut LibraryRequests) -> Option<LibraryRequest> {
        if self.load.loading() {
            return None;
        }
        Some(self.load.begin(requests))
    }

    pub(super) fn complete(
        &mut self,
        request: LibraryRequest,
        result: Result<Vec<OutputDevice>, String>,
    ) -> Option<Result<(), &str>> {
        Some(match self.load.complete(request, result)? {
            Ok(devices) => {
                self.items = devices;
                Ok(())
            }
            Err(error) => Err(error),
        })
    }
}

async fn enumerate_output_devices(
    enumerate: impl FnOnce() -> anyhow::Result<Vec<OutputDevice>> + Send + 'static,
) -> Result<Vec<OutputDevice>, String> {
    tokio::task::spawn_blocking(enumerate)
        .await
        .map_err(error_message)?
        .map_err(error_message)
}

impl NavidromeApp {
    pub(super) fn refresh_output_devices(&mut self, cx: &mut Context<Self>) {
        let Some(request) = self.state.output_devices.begin(&mut self.library_requests) else {
            return;
        };
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = enumerate_output_devices(crate::audio::list_output_devices).await;
            let _ = tx.send(Msg::OutputDevices { request, result });
        });
        cx.notify();
    }

    pub(super) fn finish_output_devices(
        &mut self,
        request: LibraryRequest,
        result: Result<Vec<OutputDevice>, String>,
    ) {
        if let Some(Err(error)) = self.state.output_devices.complete(request, result) {
            let error = error.to_string();
            self.notify_error(error);
        }
    }

    /// Audio workers resume on the chosen device at the current position and
    /// preserve pause intent; changing devices invalidates any prepared stream.
    pub(super) fn set_output_device(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        if self.config.output_device == name {
            return;
        }
        self.config.output_device = name.clone();
        if let Err(error) = config::save(&self.config) {
            self.notify_error(t!(
                "Audio output save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        self.cancel_next_preload();
        self.audio.set_output_device(name);
        cx.notify();
    }
}

#[cfg(test)]
mod tests;
