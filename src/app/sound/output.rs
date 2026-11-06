//! Output device discovery state and playback device selection.

use crate::app::library::requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use crate::app::prelude::*;
use crate::audio::OutputDevice;

#[derive(Default)]
pub(in crate::app) struct OutputDevices {
    items: Vec<OutputDevice>,
    load: LibraryLoad,
}

impl OutputDevices {
    pub(in crate::app) fn items(&self) -> &[OutputDevice] {
        &self.items
    }

    pub(in crate::app) fn loading(&self) -> bool {
        self.load.loading()
    }

    pub(in crate::app) fn begin(
        &mut self,
        requests: &mut LibraryRequests,
    ) -> Option<LibraryRequest> {
        if self.load.loading() {
            return None;
        }
        Some(self.load.begin(requests))
    }

    pub(in crate::app) fn complete(
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
    pub(in crate::app) fn refresh_output_devices(&mut self, cx: &mut Context<Self>) {
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

    pub(in crate::app) fn finish_output_devices(
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
    pub(in crate::app) fn set_output_device(
        &mut self,
        name: Option<String>,
        cx: &mut Context<Self>,
    ) {
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
