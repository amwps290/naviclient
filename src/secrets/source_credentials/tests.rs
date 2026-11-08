use super::*;
use std::{collections::HashMap, sync::Mutex};

#[derive(Default)]
struct MemoryStore {
    entries: Mutex<HashMap<String, Vec<u8>>>,
    deleted: tokio::sync::Notify,
    entered: tokio::sync::Notify,
    gate: Option<Arc<tokio::sync::Notify>>,
    fail_after_store: bool,
}

impl CredentialStore for MemoryStore {
    fn check(&self, _: bool) -> StoreFuture<'_, ()> {
        Box::pin(async { Ok(()) })
    }
    fn store<'a>(&'a self, key: &'a str, secret: &'a [u8]) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            self.entered.notify_one();
            if let Some(gate) = &self.gate {
                gate.notified().await;
            }
            self.entries
                .lock()
                .unwrap()
                .insert(key.into(), secret.to_vec());
            if self.fail_after_store {
                Err(CredentialError::Unavailable)
            } else {
                Ok(())
            }
        })
    }
    fn load<'a>(&'a self, key: &'a str) -> StoreFuture<'a, Zeroizing<Vec<u8>>> {
        Box::pin(async move {
            self.entries
                .lock()
                .unwrap()
                .get(key)
                .cloned()
                .map(Zeroizing::new)
                .ok_or(CredentialError::Missing)
        })
    }
    fn delete<'a>(&'a self, key: &'a str) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            self.entries.lock().unwrap().remove(key);
            self.deleted.notify_one();
            Ok(())
        })
    }
    fn delete_scope<'a>(&'a self, scope: &'a str) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            self.entries
                .lock()
                .unwrap()
                .retain(|key, _| !key.starts_with(scope));
            Ok(())
        })
    }
}

fn source(profile: &str) -> SourceId {
    SourceId::new(ProviderKind::Bilibili, profile)
}

#[test]
fn references_cannot_access_another_source_or_account() {
    let key = format!(
        "{}{:032x}",
        credential_scope(&source("one"), 123).unwrap(),
        1
    );
    assert!(validate_reference(&source("one"), 123, &key).is_ok());
    for (source, account_id) in [
        (source("two"), 123),
        (source("one"), 124),
        (SourceId::new(ProviderKind::OpenSubsonic, "one"), 123),
    ] {
        assert_eq!(
            validate_reference(&source, account_id, &key),
            Err(CredentialError::Invalid)
        );
    }
    assert_eq!(
        validate_reference(&source("one"), 123, &format!("{key}/../other")),
        Err(CredentialError::Invalid)
    );
}

#[tokio::test]
async fn stale_completion_deletes_only_its_staging_entry() {
    let store = Arc::new(MemoryStore::default());
    let previous = StagedCredential::create(
        store.clone(),
        &source("one"),
        123,
        Zeroizing::new(b"previous-secret".to_vec()),
    )
    .await
    .unwrap();
    let previous_key = previous.key().to_string();
    previous.commit();
    let stale = StagedCredential::create(
        store.clone(),
        &source("one"),
        123,
        Zeroizing::new(b"stale-secret".to_vec()),
    )
    .await
    .unwrap();
    let stale_key = stale.key().to_string();
    assert_ne!(previous_key, stale_key);
    assert_eq!(format!("{stale:?}"), "StagedCredential([redacted])");
    drop(stale);
    tokio::time::timeout(std::time::Duration::from_secs(2), store.deleted.notified())
        .await
        .unwrap();
    assert_eq!(
        &*store.load(&previous_key).await.unwrap(),
        b"previous-secret"
    );
    assert_eq!(
        store.load(&stale_key).await.unwrap_err(),
        CredentialError::Missing
    );
}

#[tokio::test]
async fn rejected_message_rolls_back_the_completed_store() {
    let store = Arc::new(MemoryStore::default());
    let (tx, rx) = std::sync::mpsc::channel();
    drop(rx);
    let staged = StagedCredential::create(
        store.clone(),
        &source("one"),
        123,
        Zeroizing::new(b"secret".to_vec()),
    )
    .await
    .unwrap();
    let key = staged.key().to_string();
    let _ = tx.send(staged);
    tokio::time::timeout(std::time::Duration::from_secs(2), store.deleted.notified())
        .await
        .unwrap();
    assert_eq!(
        store.load(&key).await.unwrap_err(),
        CredentialError::Missing
    );
}

#[tokio::test]
async fn cancelling_an_in_flight_store_waits_for_its_commit_then_cleans_it() {
    let gate = Arc::new(tokio::sync::Notify::new());
    let store = Arc::new(MemoryStore {
        gate: Some(gate.clone()),
        ..MemoryStore::default()
    });
    let backend = store.clone();
    let task = tokio::spawn(async move {
        StagedCredential::create(
            backend,
            &source("one"),
            123,
            Zeroizing::new(b"cancelled-secret".to_vec()),
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), store.entered.notified())
        .await
        .unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let settling = tokio::spawn(settle_staging());
    tokio::task::yield_now().await;
    assert!(!settling.is_finished());
    gate.notify_one();
    tokio::time::timeout(std::time::Duration::from_secs(2), store.deleted.notified())
        .await
        .unwrap();
    assert!(store.entries.lock().unwrap().is_empty());
    tokio::time::timeout(std::time::Duration::from_secs(2), settling)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn store_error_after_commit_rolls_back_the_ambiguous_write() {
    let store = Arc::new(MemoryStore {
        fail_after_store: true,
        ..MemoryStore::default()
    });
    let result = StagedCredential::create(
        store.clone(),
        &source("one"),
        123,
        Zeroizing::new(b"failed-secret".to_vec()),
    )
    .await;
    assert_eq!(result.unwrap_err(), CredentialError::Unavailable);
    tokio::time::timeout(std::time::Duration::from_secs(2), store.deleted.notified())
        .await
        .unwrap();
    assert!(store.entries.lock().unwrap().is_empty());
}

#[tokio::test]
#[ignore = "requires a real unlocked desktop credential store; no credentials are written"]
async fn system_credential_store_is_available_for_bilibili() {
    SystemStore.check(false).await.unwrap();
}

#[tokio::test]
async fn removing_source_also_deletes_retired_credentials_and_preserves_other_sources() {
    let store = Arc::new(MemoryStore::default());
    let mut keys = Vec::new();
    for (profile, account_id) in [("one", 123), ("one", 123), ("two", 123)] {
        let staged = StagedCredential::create(
            store.clone(),
            &source(profile),
            account_id,
            Zeroizing::new(b"fake-secret".to_vec()),
        )
        .await
        .unwrap();
        keys.push(staged.key().to_string());
        staged.commit();
    }
    store
        .delete_scope(&source_scope(&source("one")).unwrap())
        .await
        .unwrap();
    assert_eq!(
        store.load(&keys[0]).await.unwrap_err(),
        CredentialError::Missing
    );
    assert_eq!(
        store.load(&keys[1]).await.unwrap_err(),
        CredentialError::Missing
    );
    assert_eq!(&*store.load(&keys[2]).await.unwrap(), b"fake-secret");
}

#[tokio::test]
#[ignore = "writes and deletes a unique fake credential in the real system store"]
async fn system_bilibili_credential_roundtrip_and_cleanup() {
    SystemStore.check(false).await.unwrap();
    let source = source(&format!("bilibili-probe-{:032x}", rand::random::<u128>()));
    let scope = source_scope(&source).unwrap();
    let staged = StagedCredential::create(
        Arc::new(SystemStore),
        &source,
        123,
        Zeroizing::new(b"SESSDATA=fake-test-only; bili_jct=fake; DedeUserID=123".to_vec()),
    )
    .await
    .unwrap();
    let key = staged.key().to_string();
    let loaded = SystemStore.load(&key).await.unwrap();
    assert_eq!(
        &*loaded,
        b"SESSDATA=fake-test-only; bili_jct=fake; DedeUserID=123"
    );
    SystemStore.delete_scope(&scope).await.unwrap();
    assert_eq!(
        SystemStore.load(&key).await.unwrap_err(),
        CredentialError::Missing
    );
    staged.commit(); // deletion was verified explicitly; nothing remains.
}
