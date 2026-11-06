//! Server identity and separate, single-use handshake and capability requests.

use crate::app::library::requests::{LibraryLoad, LibraryRequest, LibraryRequests};
use crate::models::ServerInfo;

#[derive(Default)]
pub(in crate::app) struct ServerConnection {
    server: Option<ServerInfo>,
    ping: LibraryLoad,
    discovery: LibraryLoad,
}

impl ServerConnection {
    pub(in crate::app) fn server(&self) -> Option<&ServerInfo> {
        self.server.as_ref()
    }

    pub(in crate::app) fn loading(&self) -> bool {
        self.ping.loading()
    }

    pub(in crate::app) fn begin(&mut self, requests: &mut LibraryRequests) -> LibraryRequest {
        // A refresh invalidates the previous probe immediately, even if the
        // new handshake fails and the last server identity remains visible.
        self.discovery = LibraryLoad::default();
        self.ping.begin(requests)
    }

    pub(in crate::app) fn complete_ping(
        &mut self,
        request: LibraryRequest,
        result: Result<ServerInfo, String>,
        requests: &mut LibraryRequests,
    ) -> Option<Result<LibraryRequest, &str>> {
        Some(match self.ping.complete(request, result)? {
            Ok(server) => {
                self.server = Some(server);
                Ok(self.discovery.begin(requests))
            }
            Err(error) => Err(error),
        })
    }

    pub(in crate::app) fn complete_discovery(
        &mut self,
        request: LibraryRequest,
        result: Result<ServerInfo, String>,
    ) -> Option<Result<(), &str>> {
        Some(match self.discovery.complete(request, result)? {
            Ok(info) => {
                self.server
                    .as_mut()
                    .expect("capability discovery follows a successful handshake")
                    .merge_open_subsonic_info(info);
                Ok(())
            }
            // The optional probe must not turn a usable legacy connection
            // into a failed connection or remove its optimistic fallbacks.
            Err(error) => Err(error),
        })
    }
}

#[cfg(test)]
mod tests;
