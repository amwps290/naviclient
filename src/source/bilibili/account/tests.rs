use super::*;
use zeroize::Zeroizing;

fn session(account_id: u64) -> AuthSession {
    AuthSession::decode_secret(
        Zeroizing::new(
            format!("SESSDATA=fake-session; bili_jct=fake-csrf; DedeUserID={account_id}")
                .into_bytes(),
        ),
        account_id,
    )
    .unwrap()
}
fn account(account_id: u64) -> Account {
    Account {
        id: account_id,
        name: "listener".into(),
    }
}
fn key(source: &SourceId, account_id: u64) -> String {
    format!(
        "{}{:032x}",
        crate::secrets::source_credentials::credential_scope(source, account_id).unwrap(),
        1
    )
}

#[test]
fn reauthentication_preserves_source_and_both_playback_owners_without_serializing_cookies() {
    let mut config = Config::default();
    config.sync_active_profile();
    let browsing = config.active_source.clone();
    let playing = config.playback_source.clone();
    let source = source_for_account(&config, 123);
    let (next, previous) = prepare_config(
        &config,
        &source,
        &account(123),
        &session(123),
        &key(&source, 123),
        None,
    )
    .unwrap();
    assert!(previous.is_none());
    assert_eq!(next.active_source, browsing);
    assert_eq!(next.playback_source, playing);
    assert_eq!(source_for_account(&next, 123), source);
    assert_ne!(source_for_account(&next, 456), source);
    let json = serde_json::to_string(&next).unwrap();
    for secret in ["SESSDATA", "fake-session", "bili_jct", "fake-csrf"] {
        assert!(!json.contains(secret));
    }
    let (updated, previous) = prepare_config(
        &next,
        &source,
        &account(123),
        &session(123),
        &key(&source, 123),
        Some("http://127.0.0.1:7890".into()),
    )
    .unwrap();
    assert_eq!(previous, Some(key(&source, 123)));
    assert_eq!(updated.sources.len(), next.sources.len());
}

#[test]
fn another_account_cannot_replace_metadata_or_commit_a_foreign_reference() {
    let config = Config::default();
    let source = source_for_account(&config, 123);
    let (configured, _) = prepare_config(
        &config,
        &source,
        &account(123),
        &session(123),
        &key(&source, 123),
        None,
    )
    .unwrap();
    assert!(prepare_config(
        &configured,
        &source,
        &account(456),
        &session(456),
        &key(&source, 456),
        None
    )
    .is_err());
    assert!(prepare_config(
        &config,
        &source,
        &account(123),
        &session(456),
        &key(&source, 123),
        None
    )
    .is_err());
    let foreign = SourceId::new(ProviderKind::Bilibili, "foreign");
    assert!(prepare_config(
        &config,
        &source,
        &account(123),
        &session(123),
        &key(&foreign, 123),
        None
    )
    .is_err());
}
