//! QR task ownership is part of the settings overlay's lifetime.

use crate::models::{
    network::{NetworkError, SourceConnection},
    Config,
};
use crate::secrets::source_credentials::{CredentialError, StagedCredential};
use crate::source::{
    bilibili::{
        auth::{Account, AuthSession},
        client::BilibiliError,
    },
    SourceId, SourceProfile,
};
use gpui_kit::Image;
use std::{fmt, sync::Arc};
use tokio::task::AbortHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LoginError {
    Credentials(CredentialError),
    Protocol(BilibiliError),
    Config,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::app) enum LoginStage {
    #[default]
    Idle,
    Generating,
    WaitingScan,
    WaitingConfirmation,
    Verifying,
    Saving,
    Expired,
    Failed(LoginError),
}

pub(crate) enum LoginUpdate {
    Qr(Vec<u8>),
    Scanned,
    Verifying,
    Verified {
        account: Account,
        session: AuthSession,
    },
    Stored {
        account: Account,
        session: AuthSession,
        credential: StagedCredential,
    },
    Failed(LoginError),
    Expired,
}

impl fmt::Debug for LoginUpdate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Qr(_) => "LoginUpdate::Qr([redacted])",
            Self::Scanned => "LoginUpdate::Scanned",
            Self::Verifying => "LoginUpdate::Verifying",
            Self::Verified { .. } => "LoginUpdate::Verified([redacted])",
            Self::Stored { .. } => "LoginUpdate::Stored([redacted])",
            Self::Failed(_) => "LoginUpdate::Failed",
            Self::Expired => "LoginUpdate::Expired",
        })
    }
}

#[derive(Clone, Debug)]
pub(in crate::app) struct LoginTarget {
    pub source: SourceId,
    /// Reauthentication must verify the same account. A different account
    /// requires a new source, preserving cached identities for both accounts.
    pub account_id: Option<u64>,
    pub connection: SourceConnection,
    /// A newly added row may resolve to an already saved account during login.
    pub provisional_source: Option<SourceId>,
}

impl LoginTarget {
    /// Apply the selected connection and merge a duplicate account's new row
    /// into its saved identity as part of the same configuration transaction.
    pub(in crate::app) fn apply_source_settings(
        &self,
        config: &mut Config,
    ) -> Result<Option<SourceId>, NetworkError> {
        config.set_source_connection(&self.source, self.connection.clone())?;
        let provisional = self
            .provisional_source
            .as_ref()
            .filter(|source| *source != &self.source);
        if let Some(source) = provisional {
            // Only an unsigned row created for this login can be replaced.
            if !config.sources.iter().any(|profile| {
                profile.id() == *source
                    && matches!(
                        profile,
                        SourceProfile::Bilibili {
                            account_id: None,
                            credential_key: None,
                            ..
                        }
                    )
            }) {
                return Ok(None);
            }
            config.sources.retain(|profile| profile.id() != *source);
            if config.active_source.as_ref() == Some(source) {
                config.active_source = Some(self.source.clone());
            }
            config.sync_active_profile();
            return Ok(Some(source.clone()));
        }
        Ok(None)
    }
}

#[derive(Default)]
pub(in crate::app) struct LoginState {
    pub stage: LoginStage,
    pub image: Option<Arc<Image>>,
    pub target: Option<LoginTarget>,
    request: Option<u128>,
    task: Option<AbortHandle>,
}

impl LoginState {
    pub fn begin(&mut self, target: LoginTarget) -> u128 {
        self.cancel();
        let request = rand::random();
        self.request = Some(request);
        self.target = Some(target);
        self.stage = LoginStage::Generating;
        request
    }
    pub fn accepts(&self, request: u128) -> bool {
        self.request == Some(request)
    }
    pub fn attach(&mut self, request: u128, task: AbortHandle) {
        if self.accepts(request) {
            self.task = Some(task);
        } else {
            task.abort();
        }
    }
    pub fn finish_polling(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.image = None;
    }
    pub fn cancel(&mut self) {
        self.finish_polling();
        self.request = None;
        self.target = None;
        self.stage = LoginStage::Idle;
    }
}

impl Drop for LoginState {
    fn drop(&mut self) {
        self.cancel();
    }
}

#[derive(Debug)]
pub(in crate::app) enum AccountStatus {
    Checking { request: u128 },
    Ready(AuthSession),
    Failed(LoginError),
    Removing,
}

impl AccountStatus {
    pub(in crate::app) fn complete_check(
        &mut self,
        request: u128,
        result: Result<AuthSession, LoginError>,
    ) -> bool {
        if !matches!(self, Self::Checking { request: current } if *current == request) {
            return false;
        }
        *self = match result {
            Ok(session) => Self::Ready(session),
            Err(error) => Self::Failed(error),
        };
        true
    }
}

#[cfg(test)]
mod tests;
