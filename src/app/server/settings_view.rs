//! Protocol details of the connected Navidrome server, for its source row.

use crate::app::prelude::*;

impl NavidromeApp {
    /// Server name/version for the row, plus protocol and extension details
    /// for its tooltip. `None` until the browsing server is connected.
    pub(in crate::app) fn connected_server_summary(&self) -> Option<(String, String)> {
        let server = self.state.connection.server()?;
        let name = format!(
            "{} {}",
            server.app_name.as_deref().unwrap_or("Navidrome"),
            server.display_version()
        );
        let protocol = if server.extensions_discovered {
            t!(
                "OpenSubsonic %{version} · %{count} extensions",
                version = server.protocol_version,
                count = server.extensions.len()
            )
            .to_string()
        } else {
            t!(
                "Subsonic %{version} · compatibility mode",
                version = server.protocol_version
            )
            .to_string()
        };
        let details = match server.extension_summary(6) {
            Some(extensions) if server.capabilities.smart_discovery().is_supported() => format!(
                "{protocol}\n{}",
                t!(
                    "Smart discovery available · %{extensions}",
                    extensions = extensions
                )
            ),
            Some(extensions) => format!(
                "{protocol}\n{}",
                t!("Capabilities: %{extensions}", extensions = extensions)
            ),
            None => protocol,
        };
        Some((name, details))
    }
}
