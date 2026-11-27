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

/// Short timeout for optional Navidrome password keyring operations; if the
/// keyring is absent or locked, callers fall back to config storage cleanly.
const SERVER_KEYRING_TIMEOUT: Duration = Duration::from_secs(3);

fn server_attributes(target: &str) -> HashMap<&str, &str> {
    HashMap::from([
        ("application", "navidrome-client"),
        ("provider", "opensubsonic"),
        ("target", target),
    ])
}

fn run_sync<T>(fut: impl std::future::Future<Output = T> + Send + 'static) -> Option<T>
where
    T: Send + 'static,
{
    std::thread::Builder::new()
        .name("linux-server-keyring".into())
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .ok()
                .map(|rt| rt.block_on(fut))
        })
        .ok()
        .and_then(|handle| handle.join().ok())
        .flatten()
}

pub(super) fn store_server_password(
    target: &str,
    username: &str,
    password: &str,
) -> anyhow::Result<()> {
    let target = target.to_string();
    let username = username.to_string();
    let password = password.to_string();
    let result = run_sync(async move {
        tokio::time::timeout(SERVER_KEYRING_TIMEOUT, async move {
            let service = SecretService::connect(EncryptionType::Dh).await?;
            let collection = service.get_default_collection().await?;
            if collection.is_locked().await? {
                return Err(secret_service::Error::Locked);
            }
            let payload = serde_json::json!({
                "username": username,
                "password": password,
            })
            .to_string();
            collection
                .create_item(
                    &format!("Navidrome Client · {target}"),
                    server_attributes(&target),
                    payload.as_bytes(),
                    true,
                    "application/json; charset=utf-8",
                )
                .await?;
            Ok::<_, secret_service::Error>(())
        })
        .await
    });
    match result {
        Some(Ok(Ok(()))) => Ok(()),
        Some(Ok(Err(error))) => Err(anyhow::anyhow!("linux secret service error: {error}")),
        Some(Err(_)) | None => Err(anyhow::anyhow!("linux secret service unavailable")),
    }
}

pub(super) fn load_server_password(target: &str) -> Option<(String, String)> {
    let target = target.to_string();
    run_sync(async move {
        tokio::time::timeout(SERVER_KEYRING_TIMEOUT, async move {
            let service = SecretService::connect(EncryptionType::Dh).await.ok()?;
            let items = service
                .search_items(server_attributes(&target))
                .await
                .ok()?;
            if !items.locked.is_empty() {
                return None;
            }
            let item = items.unlocked.first()?;
            let secret = Zeroizing::new(item.get_secret().await.ok()?);
            let value: serde_json::Value = serde_json::from_slice(&secret).ok()?;
            let username = value.get("username")?.as_str()?.to_string();
            let password = value.get("password")?.as_str()?.to_string();
            Some((username, password))
        })
        .await
        .ok()
        .flatten()
    })
    .flatten()
}

pub(super) fn delete_server_password(target: &str) -> bool {
    let target = target.to_string();
    run_sync(async move {
        tokio::time::timeout(SERVER_KEYRING_TIMEOUT, async move {
            let service = SecretService::connect(EncryptionType::Dh).await.ok()?;
            let items = service
                .search_items(server_attributes(&target))
                .await
                .ok()?;
            if !items.locked.is_empty() || items.unlocked.is_empty() {
                return Some(false);
            }
            let mut deleted = false;
            for item in items.unlocked {
                if item.delete().await.is_ok() {
                    deleted = true;
                }
            }
            Some(deleted)
        })
        .await
        .ok()
        .flatten()
        .unwrap_or(false)
    })
    .unwrap_or(false)
}
