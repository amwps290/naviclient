//! Strict source-qualified credentials. There is deliberately no file fallback.

use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use crate::source::{ProviderKind, SourceId};
use zeroize::Zeroizing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CredentialError {
    Missing,
    Locked,
    Unavailable,
    Invalid,
}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Missing => "credential is missing",
            Self::Locked => "credential store is locked",
            Self::Unavailable => "credential store is unavailable",
            Self::Invalid => "credential is invalid",
        })
    }
}

impl std::error::Error for CredentialError {}

pub type StoreResult<T> = Result<T, CredentialError>;
pub type StoreFuture<'a, T> = Pin<Box<dyn Future<Output = StoreResult<T>> + Send + 'a>>;

pub trait CredentialStore: Send + Sync {
    fn check(&self, unlock: bool) -> StoreFuture<'_, ()>;
    fn store<'a>(&'a self, key: &'a str, secret: &'a [u8]) -> StoreFuture<'a, ()>;
    fn load<'a>(&'a self, key: &'a str) -> StoreFuture<'a, Zeroizing<Vec<u8>>>;
    fn delete<'a>(&'a self, key: &'a str) -> StoreFuture<'a, ()>;
    fn delete_scope<'a>(&'a self, scope: &'a str) -> StoreFuture<'a, ()>;
}

pub struct SystemStore;

static ACTIVE_STAGING: AtomicUsize = AtomicUsize::new(0);
static STAGING_SETTLED: tokio::sync::Notify = tokio::sync::Notify::const_new();

struct StagingActivity;
impl StagingActivity {
    fn new() -> Self {
        ACTIVE_STAGING.fetch_add(1, Ordering::AcqRel);
        Self
    }
}
impl Drop for StagingActivity {
    fn drop(&mut self) {
        if ACTIVE_STAGING.fetch_sub(1, Ordering::AcqRel) == 1 {
            STAGING_SETTLED.notify_waiters();
        }
    }
}

/// After closing login result receivers, graceful exit lets pending writes
/// finish and roll back before destroying the runtime that performs cleanup.
pub async fn settle_staging() {
    loop {
        let notified = STAGING_SETTLED.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if ACTIVE_STAGING.load(Ordering::Acquire) == 0 {
            return;
        }
        notified.await;
    }
}

impl CredentialStore for SystemStore {
    fn check(&self, unlock: bool) -> StoreFuture<'_, ()> {
        Box::pin(async move {
            #[cfg(target_os = "linux")]
            {
                super::linux_credentials::check(unlock).await
            }
            #[cfg(target_os = "windows")]
            {
                let _ = unlock;
                Ok(())
            }
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            {
                let _ = unlock;
                Err(CredentialError::Unavailable)
            }
        })
    }

    fn store<'a>(&'a self, key: &'a str, secret: &'a [u8]) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            #[cfg(target_os = "linux")]
            {
                super::linux_credentials::store(key, secret).await
            }
            #[cfg(target_os = "windows")]
            {
                super::windows_credentials::store_source(key, secret)
            }
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            {
                let _ = (key, secret);
                Err(CredentialError::Unavailable)
            }
        })
    }

    fn load<'a>(&'a self, key: &'a str) -> StoreFuture<'a, Zeroizing<Vec<u8>>> {
        Box::pin(async move {
            #[cfg(target_os = "linux")]
            {
                super::linux_credentials::load(key).await
            }
            #[cfg(target_os = "windows")]
            {
                super::windows_credentials::load_source(key)
            }
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            {
                let _ = key;
                Err(CredentialError::Unavailable)
            }
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            #[cfg(target_os = "linux")]
            {
                super::linux_credentials::delete(key).await
            }
            #[cfg(target_os = "windows")]
            {
                super::windows_credentials::delete_source(key)
            }
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            {
                let _ = key;
                Err(CredentialError::Unavailable)
            }
        })
    }

    fn delete_scope<'a>(&'a self, scope: &'a str) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            #[cfg(target_os = "linux")]
            {
                super::linux_credentials::delete_scope(scope).await
            }
            #[cfg(target_os = "windows")]
            {
                super::windows_credentials::delete_source_scope(scope)
            }
            #[cfg(not(any(target_os = "linux", target_os = "windows")))]
            {
                let _ = scope;
                Err(CredentialError::Unavailable)
            }
        })
    }
}

pub fn credential_scope(source: &SourceId, account_id: u64) -> StoreResult<String> {
    if source.provider != ProviderKind::Bilibili || source.profile_id.is_empty() || account_id == 0
    {
        return Err(CredentialError::Invalid);
    }
    Ok(format!("{}{account_id}/", source_scope(source)?))
}

pub fn source_scope(source: &SourceId) -> StoreResult<String> {
    if source.provider != ProviderKind::Bilibili || source.profile_id.is_empty() {
        return Err(CredentialError::Invalid);
    }
    Ok(format!(
        "navidrome-client/bilibili/{}/",
        source.cache_namespace()
    ))
}

/// A config reference can only address this account in this source namespace.
pub fn validate_reference(source: &SourceId, account_id: u64, key: &str) -> StoreResult<()> {
    let prefix = credential_scope(source, account_id)?;
    let nonce = key.strip_prefix(&prefix).ok_or(CredentialError::Invalid)?;
    if nonce.len() != 32 || !nonce.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(CredentialError::Invalid);
    }
    Ok(())
}

/// Unique staging entries cannot overwrite a previous login. The store task
/// must run to completion once it starts writing; only its UI ownership is
/// canceled. Dropping a stale completion rolls back its own entry.
pub struct StagedCredential {
    key: String,
    store: Arc<dyn CredentialStore>,
    runtime: tokio::runtime::Handle,
    committed: bool,
    activity: Option<StagingActivity>,
}

impl fmt::Debug for StagedCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StagedCredential([redacted])")
    }
}

impl StagedCredential {
    pub async fn create(
        store: Arc<dyn CredentialStore>,
        source: &SourceId,
        account_id: u64,
        secret: Zeroizing<Vec<u8>>,
    ) -> StoreResult<Self> {
        let key = format!(
            "{}{:032x}",
            credential_scope(source, account_id)?,
            rand::random::<u128>()
        );
        let staged = Self {
            key,
            store,
            runtime: tokio::runtime::Handle::current(),
            committed: false,
            activity: Some(StagingActivity::new()),
        };
        // Bound the caller's wait without canceling an in-flight OS mutation.
        // A closed receiver drops the eventual result and rolls it back.
        static STORE_SLOT: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
        let permit = tokio::time::timeout(std::time::Duration::from_secs(30), STORE_SLOT.acquire())
            .await
            .map_err(|_| CredentialError::Unavailable)?
            .map_err(|_| CredentialError::Unavailable)?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let _permit = permit;
            let result = staged.store.store(&staged.key, &secret).await;
            let result = result.map(|()| staged);
            let _ = tx.send(result);
        });
        tokio::time::timeout(std::time::Duration::from_secs(30), rx)
            .await
            .map_err(|_| CredentialError::Unavailable)?
            .map_err(|_| CredentialError::Unavailable)?
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    /// Called only after the atomic configuration write has succeeded.
    pub fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for StagedCredential {
    fn drop(&mut self) {
        if !self.committed {
            let store = self.store.clone();
            let key = self.key.clone();
            let activity = self.activity.take();
            self.runtime.spawn(async move {
                let _activity = activity;
                if store.delete(&key).await.is_err() {
                    log::warn!("could not remove staged Bilibili credential");
                }
            });
        }
    }
}

#[cfg(test)]
mod tests;
