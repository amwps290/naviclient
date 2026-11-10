use super::*;
use crate::source::ProviderKind;

fn target() -> LoginTarget {
    LoginTarget {
        source: SourceId::new(ProviderKind::Bilibili, "account"),
        account_id: Some(123),
        connection: Default::default(),
        provisional_source: None,
    }
}

#[test]
fn duplicate_account_login_keeps_the_selected_connection_and_saved_identity() {
    use crate::models::Config;
    use crate::source::{ProviderKind, SourceProfile};
    let existing = crate::source::SourceId::new(ProviderKind::Bilibili, "existing");
    let pending = crate::source::SourceId::new(ProviderKind::Bilibili, "pending");
    let mut config = Config {
        sources: vec![
            SourceProfile::Bilibili {
                id: "existing".into(),
                name: "account".into(),
                account_id: Some(123),
                credential_key: Some("saved-credential".into()),
                proxy: None,
            },
            SourceProfile::Bilibili {
                id: "pending".into(),
                name: "Bilibili".into(),
                account_id: None,
                credential_key: None,
                proxy: None,
            },
        ],
        active_source: Some(pending.clone()),
        playback_source: Some(existing.clone()),
        ..Default::default()
    };
    let first = config
        .store_proxy(None, "First", "http://127.0.0.1:7890")
        .unwrap();
    let selected = config
        .store_proxy(None, "Selected", "http://127.0.0.1:7890")
        .unwrap();
    use crate::models::network::SourceConnection;
    config
        .set_source_connection(&existing, SourceConnection::Proxy(first))
        .unwrap();
    config
        .set_source_connection(&pending, SourceConnection::Proxy(selected.clone()))
        .unwrap();
    for connection in [
        SourceConnection::Proxy(selected.clone()),
        SourceConnection::Direct,
        SourceConnection::System,
    ] {
        let mut next = config.clone();
        let target = LoginTarget {
            source: existing.clone(),
            account_id: Some(123),
            connection: connection.clone(),
            provisional_source: Some(pending.clone()),
        };
        assert_eq!(
            target.apply_source_settings(&mut next).unwrap(),
            Some(pending.clone())
        );
        assert_eq!(next.sources.len(), 1);
        assert_eq!(next.sources[0], config.sources[0]);
        assert_eq!(next.active_source.as_ref(), Some(&existing));
        assert_eq!(next.playback_source.as_ref(), Some(&existing));
        assert_eq!(next.source_connection(&existing), connection);
        assert!(next
            .network
            .source_proxies
            .iter()
            .all(|entry| entry.source != pending));
        assert_eq!(next.network.proxies.len(), 2);
    }
}

#[test]
fn changing_proxy_rejects_old_account_checks_and_late_checks_cannot_replace_login_or_removal() {
    let network_error = LoginError::Protocol(BilibiliError::Network);
    let mut status = AccountStatus::Checking { request: 2 };
    assert!(!status.complete_check(1, Err(network_error)));
    assert!(matches!(status, AccountStatus::Checking { request: 2 }));
    let session = AuthSession::decode_secret(
        zeroize::Zeroizing::new(b"SESSDATA=fake; bili_jct=csrf; DedeUserID=123".to_vec()),
        123,
    )
    .unwrap();
    assert!(status.complete_check(2, Ok(session)));
    assert!(!status.complete_check(2, Err(network_error)));
    assert!(matches!(status, AccountStatus::Ready(session) if session.account_id() == 123));
    let mut failed = AccountStatus::Checking { request: 3 };
    assert!(failed.complete_check(3, Err(network_error)));
    assert!(matches!(failed, AccountStatus::Failed(error) if error == network_error));
    let mut removing = AccountStatus::Removing;
    assert!(!removing.complete_check(3, Err(network_error)));
    assert!(matches!(removing, AccountStatus::Removing));
}

#[tokio::test]
async fn refreshing_or_closing_settings_aborts_polling_and_rejects_late_results() {
    let mut settings = crate::app::settings::state::SettingsUi::default();
    settings.open = true;
    let first = settings.bilibili_login.begin(target());
    let task = tokio::spawn(std::future::pending::<()>());
    settings.bilibili_login.attach(first, task.abort_handle());
    let second = settings.bilibili_login.begin(target());
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(!settings.bilibili_login.accepts(first));
    assert!(settings.bilibili_login.accepts(second));
    let task = tokio::spawn(std::future::pending::<()>());
    settings.bilibili_login.attach(second, task.abort_handle());
    settings.close();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(!settings.bilibili_login.accepts(second));
    assert!(settings.bilibili_login.target.is_none());
    assert_eq!(settings.bilibili_login.stage, LoginStage::Idle);
}

#[tokio::test]
async fn navigation_and_escape_cancel_login_without_changing_the_queue() {
    let mut state = crate::app::shell::state::AppState::default();
    state.settings.open = true;
    let request = state.settings.bilibili_login.begin(target());
    state.navigate_to(crate::app::shell::view_state::View::Home);
    assert!(!state.settings.bilibili_login.accepts(request));
    state.settings.open = true;
    let request = state.settings.bilibili_login.begin(target());
    state.close_top_overlay();
    assert!(!state.settings.bilibili_login.accepts(request));
    assert!(state.queue.is_empty());
}
