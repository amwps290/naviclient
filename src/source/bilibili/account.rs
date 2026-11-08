//! Account metadata transactions shared by native and remote login.

use super::auth::{Account, AuthSession};
use crate::models::Config;
use crate::secrets::source_credentials::{validate_reference, CredentialError};
use crate::source::{ProviderKind, SourceId, SourceProfile};

/// Startup and SSH diagnostics share the same strict, read-only restore path.
pub(crate) async fn restore_session(profile: &SourceProfile) -> anyhow::Result<AuthSession> {
    use crate::secrets::source_credentials::{CredentialStore, SystemStore};
    let SourceProfile::Bilibili {
        account_id: Some(account_id),
        credential_key: Some(key),
        proxy,
        ..
    } = profile
    else {
        return Err(CredentialError::Missing.into());
    };
    validate_reference(&profile.id(), *account_id, key)?;
    let secret = SystemStore.load(key).await?;
    let session = AuthSession::decode_secret(secret, *account_id)?;
    super::client::BilibiliClient::new(proxy.as_deref())?
        .verify_account(&session)
        .await?;
    Ok(session)
}

pub(crate) fn source_for_account(config: &Config, account_id: u64) -> SourceId {
    config.sources.iter().find(|profile| matches!(profile, SourceProfile::Bilibili { account_id: Some(id), .. } if *id == account_id))
        .map(SourceProfile::id)
        .unwrap_or_else(|| SourceId::new(ProviderKind::Bilibili, format!("{:032x}", rand::random::<u128>())))
}

pub(crate) fn prepare_config(
    config: &Config,
    source: &SourceId,
    account: &Account,
    session: &AuthSession,
    key: &str,
    proxy: Option<String>,
) -> Result<(Config, Option<String>), CredentialError> {
    validate_reference(source, account.id, key)?;
    if session.account_id() != account.id {
        return Err(CredentialError::Invalid);
    }
    let previous = config
        .sources
        .iter()
        .find(|profile| profile.id() == *source);
    if previous.is_some_and(|profile| matches!(profile, SourceProfile::Bilibili { account_id: Some(id), .. } if *id != account.id)) {
        return Err(CredentialError::Invalid);
    }
    let old_key = previous.and_then(|profile| match profile {
        SourceProfile::Bilibili { credential_key, .. } => credential_key.clone(),
        _ => None,
    });
    let profile = SourceProfile::Bilibili {
        id: source.profile_id.clone(),
        name: account.name.clone(),
        account_id: Some(account.id),
        credential_key: Some(key.to_string()),
        proxy,
    };
    let mut next = config.clone();
    if let Some(previous) = next
        .sources
        .iter_mut()
        .find(|profile| profile.id() == *source)
    {
        *previous = profile;
    } else {
        next.sources.push(profile);
    }
    Ok((next, old_key))
}

#[cfg(test)]
mod tests;
