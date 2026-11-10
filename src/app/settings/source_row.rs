//! What one music-source row shows, derived without touching the UI.

use rust_i18n::t;

use super::bilibili_state::{AccountStatus, LoginError};
use crate::models::Config;
use crate::secrets::source_credentials::CredentialError;
use crate::source::bilibili::client::BilibiliError;
use crate::source::{ProviderKind, SourceProfile};

/// Colour of the status dot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum SourceHealth {
    Connected,
    Busy,
    Problem,
    Idle,
}

/// The one action a row offers inline; everything else lives in its menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::app) enum SourceAction {
    SetUp,
    SignIn,
    Retry,
    Switch,
}

/// The browsing connection, as far as one row is concerned.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::app) struct Connection {
    pub(in crate::app) current: bool,
    pub(in crate::app) loading: bool,
    /// Server name and version once connected.
    pub(in crate::app) connected: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::app) struct SourceRow {
    pub(in crate::app) label: String,
    pub(in crate::app) provider: ProviderKind,
    pub(in crate::app) detail: String,
    pub(in crate::app) health: SourceHealth,
    pub(in crate::app) action: Option<SourceAction>,
    /// Account work is in flight; editing and removal wait for it.
    pub(in crate::app) busy: bool,
}

impl SourceRow {
    pub(in crate::app) fn derive(
        config: &Config,
        profile: &SourceProfile,
        account: Option<&AccountStatus>,
        connection: Connection,
    ) -> Option<Self> {
        match profile {
            SourceProfile::OpenSubsonic { server_id } => {
                let server = config
                    .servers
                    .iter()
                    .find(|server| &server.id == server_id)?;
                let (health, detail, action) = if !server.is_configured() {
                    (
                        SourceHealth::Idle,
                        t!("Not configured yet").to_string(),
                        Some(SourceAction::SetUp),
                    )
                } else if !connection.current {
                    (
                        SourceHealth::Idle,
                        format!("{} · {}", server.host(), server.username),
                        Some(SourceAction::Switch),
                    )
                } else if connection.loading {
                    (SourceHealth::Busy, t!("Connecting…").to_string(), None)
                } else if let Some(server) = connection.connected {
                    (
                        SourceHealth::Connected,
                        t!("Connected · %{server}", server = server).to_string(),
                        None,
                    )
                } else {
                    (
                        SourceHealth::Problem,
                        t!("Not connected").to_string(),
                        Some(SourceAction::Retry),
                    )
                };
                Some(Self {
                    label: server.label(),
                    provider: ProviderKind::OpenSubsonic,
                    detail,
                    health,
                    action,
                    busy: false,
                })
            }
            SourceProfile::Bilibili {
                name, account_id, ..
            } => {
                let label = if name.trim().is_empty() {
                    "Bilibili".to_string()
                } else {
                    name.trim().to_string()
                };
                let (health, detail, action, busy) = match (account_id, account) {
                    (None, _) => (
                        SourceHealth::Idle,
                        t!("Sign in to Bilibili").to_string(),
                        Some(SourceAction::SignIn),
                        false,
                    ),
                    (Some(id), Some(AccountStatus::Ready(session)))
                        if session.account_id() == *id =>
                    {
                        if !connection.current {
                            (
                                SourceHealth::Connected,
                                t!("Signed in").to_string(),
                                Some(SourceAction::Switch),
                                false,
                            )
                        } else if connection.loading {
                            (
                                SourceHealth::Busy,
                                t!("Connecting…").to_string(),
                                None,
                                false,
                            )
                        } else if connection.connected.is_some() {
                            (
                                SourceHealth::Connected,
                                t!("Signed in").to_string(),
                                None,
                                false,
                            )
                        } else {
                            (
                                SourceHealth::Problem,
                                t!("Not connected").to_string(),
                                Some(SourceAction::Retry),
                                false,
                            )
                        }
                    }
                    (_, Some(AccountStatus::Checking { .. })) => (
                        SourceHealth::Busy,
                        t!("Verifying Bilibili account…").to_string(),
                        None,
                        true,
                    ),
                    (_, Some(AccountStatus::Removing)) => (
                        SourceHealth::Busy,
                        t!("Removing Bilibili account…").to_string(),
                        None,
                        true,
                    ),
                    (_, Some(AccountStatus::Failed(error))) => (
                        SourceHealth::Problem,
                        error.message().to_string(),
                        Some(if error.needs_login() {
                            SourceAction::SignIn
                        } else {
                            SourceAction::Retry
                        }),
                        false,
                    ),
                    // A signed-in source whose session belongs to another
                    // account, or that has not been checked yet.
                    _ => (
                        SourceHealth::Problem,
                        t!("Sign in to Bilibili").to_string(),
                        Some(SourceAction::SignIn),
                        false,
                    ),
                };
                Some(Self {
                    label,
                    provider: ProviderKind::Bilibili,
                    detail,
                    health,
                    action,
                    busy,
                })
            }
        }
    }
}

impl LoginError {
    /// Only signing in again can fix these; retrying the check cannot.
    pub(in crate::app) fn needs_login(self) -> bool {
        matches!(
            self,
            Self::Credentials(CredentialError::Missing | CredentialError::Invalid)
                | Self::Protocol(BilibiliError::NotAuthenticated | BilibiliError::Api(-101))
        )
    }
}

#[cfg(test)]
mod tests;
