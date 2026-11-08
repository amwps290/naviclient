//! Server handshake and optional capability discovery orchestration.

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn refresh_connection(&mut self) {
        let Some(api) = self.api.clone() else { return };
        let request = self.state.connection.begin(&mut self.library_requests);
        self.state.status = t!("Connecting to server...").to_string();
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.ping().await.map_err(error_message);
            let _ = tx.send(Msg::Ping { request, result });
        });
    }

    fn load_open_subsonic_info(&mut self, request: LibraryRequest) {
        let Some(api) = self.api.clone() else { return };
        if api.id().provider != crate::source::ProviderKind::OpenSubsonic {
            return;
        }
        let tx = self.tx.clone();
        self.spawn_future(async move {
            let result = api.open_subsonic_info().await.map_err(error_message);
            let _ = tx.send(Msg::OpenSubsonicInfo { request, result });
        });
    }

    pub(in crate::app) fn finish_ping(
        &mut self,
        request: LibraryRequest,
        result: Result<ServerInfo, String>,
    ) {
        let Some(result) =
            self.state
                .connection
                .complete_ping(request, result, &mut self.library_requests)
        else {
            return;
        };
        match result {
            Ok(discovery) => {
                self.update_connection_status();
                self.load_open_subsonic_info(discovery);
            }
            Err(error) => {
                let notification = t!("Connection failed: %{error}", error = error).to_string();
                self.state.status = t!("Connection failed").to_string();
                self.notify_error(notification);
            }
        }
    }

    pub(in crate::app) fn finish_open_subsonic_info(
        &mut self,
        request: LibraryRequest,
        result: Result<ServerInfo, String>,
    ) {
        let Some(result) = self.state.connection.complete_discovery(request, result) else {
            return;
        };
        match result {
            Ok(()) => self.update_connection_status(),
            Err(error) => log::debug!("OpenSubsonic capability discovery unavailable; using Subsonic compatibility mode: {error}"),
        }
    }

    fn update_connection_status(&mut self) {
        if self
            .api
            .as_ref()
            .zip(self.playback_source.as_ref())
            .is_some_and(|(browse, play)| {
                browse.id() == play.id() && browse.base_url() == play.base_url()
            })
        {
            self.playback_server = self.state.connection.server().cloned();
        }
        let Some(server) = self.state.connection.server() else {
            return;
        };
        let base_url = self
            .api
            .as_ref()
            .map(MusicSource::base_url)
            .unwrap_or_default();
        self.state.status = format!(
            "{} {} - {}",
            server.app_name.as_deref().unwrap_or("Navidrome"),
            server.display_version(),
            base_url
        );
    }
}
