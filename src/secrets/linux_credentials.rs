//! Secret Service with DH encryption; backend errors never expose messages.

use super::source_credentials::{CredentialError, StoreResult};
use secret_service::{EncryptionType, SecretService};
use std::{collections::HashMap, time::Duration};
use zeroize::Zeroizing;

fn classify(error: secret_service::Error) -> CredentialError {
    match error {
        secret_service::Error::Locked | secret_service::Error::Prompt => CredentialError::Locked,
        _ => CredentialError::Unavailable,
    }
}

async fn connect() -> StoreResult<SecretService<'static>> {
    tokio::time::timeout(
        Duration::from_secs(15),
        SecretService::connect(EncryptionType::Dh),
    )
    .await
    .map_err(|_| CredentialError::Unavailable)?
    .map_err(classify)
}

fn attributes(key: &str) -> HashMap<&str, &str> {
    let scope = key.rsplit_once('/').map(|(scope, _)| scope).unwrap_or("");
    let source = scope
        .rsplit_once('/')
        .map(|(source, _)| source)
        .unwrap_or("");
    HashMap::from([
        ("application", "navidrome-client"),
        ("provider", "bilibili"),
        ("reference", key),
        ("scope", scope),
        ("source", source),
    ])
}

pub(super) async fn check(unlock: bool) -> StoreResult<()> {
    tokio::time::timeout(Duration::from_secs(30), async {
        let service = connect().await?;
        let collection = service.get_default_collection().await.map_err(classify)?;
        if collection.is_locked().await.map_err(classify)? {
            if !unlock {
                return Err(CredentialError::Locked);
            }
            collection.unlock().await.map_err(classify)?;
            collection.ensure_unlocked().await.map_err(classify)?;
        }
        Ok(())
    })
    .await
    .map_err(|_| CredentialError::Unavailable)?
}

pub(super) async fn store(key: &str, secret: &[u8]) -> StoreResult<()> {
    let service = connect().await?;
    let collection = service.get_default_collection().await.map_err(classify)?;
    collection.ensure_unlocked().await.map_err(classify)?;
    // Mutation isn't aborted midway: a timed-out CreateItem can still commit.
    collection
        .create_item(
            "Naviclient · Bilibili",
            attributes(key),
            secret,
            false,
            "text/plain; charset=utf-8",
        )
        .await
        .map_err(classify)?;
    Ok(())
}

pub(super) async fn load(key: &str) -> StoreResult<Zeroizing<Vec<u8>>> {
    tokio::time::timeout(Duration::from_secs(30), async {
        let service = connect().await?;
        let items = service
            .search_items(attributes(key))
            .await
            .map_err(classify)?;
        if !items.locked.is_empty() {
            return Err(CredentialError::Locked);
        }
        if items.unlocked.len() > 1 {
            return Err(CredentialError::Invalid);
        }
        let item = items.unlocked.first().ok_or(CredentialError::Missing)?;
        Ok(Zeroizing::new(item.get_secret().await.map_err(classify)?))
    })
    .await
    .map_err(|_| CredentialError::Unavailable)?
}

pub(super) async fn delete(key: &str) -> StoreResult<()> {
    let service = connect().await?;
    let items = service
        .search_items(attributes(key))
        .await
        .map_err(classify)?;
    if !items.locked.is_empty() {
        return Err(CredentialError::Locked);
    }
    for item in items.unlocked {
        item.delete().await.map_err(classify)?;
    }
    Ok(())
}

pub(super) async fn delete_scope(scope: &str) -> StoreResult<()> {
    let service = connect().await?;
    let items = service
        .search_items(HashMap::from([
            ("application", "navidrome-client"),
            ("provider", "bilibili"),
            ("source", scope.trim_end_matches('/')),
        ]))
        .await
        .map_err(classify)?;
    if !items.locked.is_empty() {
        return Err(CredentialError::Locked);
    }
    for item in items.unlocked {
        item.delete().await.map_err(classify)?;
    }
    Ok(())
}
