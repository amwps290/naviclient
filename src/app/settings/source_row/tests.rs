use super::*;
use crate::models::{Config, ServerProfile};
use crate::secrets::source_credentials::CredentialError;
use crate::source::bilibili::auth::AuthSession;
use crate::source::bilibili::client::BilibiliError;
use crate::source::ProviderKind;

fn session(account_id: u64) -> AuthSession {
    AuthSession::decode_secret(
        zeroize::Zeroizing::new(
            format!("SESSDATA=fake; bili_jct=csrf; DedeUserID={account_id}").into_bytes(),
        ),
        account_id,
    )
    .unwrap()
}

fn bilibili(account_id: Option<u64>) -> SourceProfile {
    SourceProfile::Bilibili {
        id: "account".into(),
        name: "listener".into(),
        account_id,
        credential_key: account_id.map(|_| "credential".into()),
        proxy: None,
    }
}

fn server(configured: bool) -> (Config, SourceProfile) {
    let profile = if configured {
        ServerProfile {
            id: "home".into(),
            name: "Home".into(),
            ..ServerProfile::new("https://music.example", "listener", "secret")
        }
    } else {
        ServerProfile {
            id: "home".into(),
            name: "Home".into(),
            ..ServerProfile::new("", "", "")
        }
    };
    let config = Config {
        servers: vec![profile],
        ..Default::default()
    };
    let source = SourceProfile::OpenSubsonic {
        server_id: "home".into(),
    };
    (config, source)
}

const IDLE: Connection = Connection {
    current: false,
    loading: false,
    connected: None,
};

#[test]
fn servers_that_are_not_current_offer_a_switch_or_setup() {
    let (config, profile) = server(true);
    let row = SourceRow::derive(&config, &profile, None, IDLE).unwrap();
    assert_eq!(row.label, "Home");
    assert_eq!(row.provider, ProviderKind::OpenSubsonic);
    assert_eq!(row.health, SourceHealth::Idle);
    assert_eq!(row.action, Some(SourceAction::Switch));
    assert!(row.detail.contains("music.example"));
    let (config, profile) = server(false);
    let row = SourceRow::derive(&config, &profile, None, IDLE).unwrap();
    assert_eq!(row.action, Some(SourceAction::SetUp));
}

#[test]
fn the_current_server_reports_connecting_connected_or_retry() {
    let (config, profile) = server(true);
    let current = |loading, connected: Option<&str>| Connection {
        current: true,
        loading,
        connected: connected.map(str::to_owned),
    };
    let row = SourceRow::derive(&config, &profile, None, current(true, None)).unwrap();
    assert_eq!((row.health, row.action), (SourceHealth::Busy, None));
    let row = SourceRow::derive(
        &config,
        &profile,
        None,
        current(false, Some("Navidrome 0.53")),
    )
    .unwrap();
    assert_eq!((row.health, row.action), (SourceHealth::Connected, None));
    assert!(row.detail.contains("Navidrome 0.53"));
    let row = SourceRow::derive(&config, &profile, None, current(false, None)).unwrap();
    assert_eq!(
        (row.health, row.action),
        (SourceHealth::Problem, Some(SourceAction::Retry))
    );
}

#[test]
fn bilibili_rows_follow_the_account_state() {
    let config = Config::default();
    let row = SourceRow::derive(&config, &bilibili(None), None, IDLE).unwrap();
    assert_eq!(row.provider, ProviderKind::Bilibili);
    assert_eq!(row.action, Some(SourceAction::SignIn));
    let ready = AccountStatus::Ready(session(123));
    let row = SourceRow::derive(&config, &bilibili(Some(123)), Some(&ready), IDLE).unwrap();
    assert_eq!(
        (row.health, row.action),
        (SourceHealth::Connected, Some(SourceAction::Switch))
    );
    let current = Connection {
        current: true,
        loading: false,
        connected: Some("Bilibili".into()),
    };
    let row = SourceRow::derive(&config, &bilibili(Some(123)), Some(&ready), current).unwrap();
    assert_eq!((row.health, row.action), (SourceHealth::Connected, None));
    // A session for another account is not this source's session.
    let other = AccountStatus::Ready(session(456));
    let row = SourceRow::derive(&config, &bilibili(Some(123)), Some(&other), IDLE).unwrap();
    assert_ne!(row.health, SourceHealth::Connected);
    let checking = AccountStatus::Checking { request: 1 };
    let row = SourceRow::derive(&config, &bilibili(Some(123)), Some(&checking), IDLE).unwrap();
    assert_eq!((row.health, row.action), (SourceHealth::Busy, None));
    assert!(row.busy);
    let removing = AccountStatus::Removing;
    let row = SourceRow::derive(&config, &bilibili(Some(123)), Some(&removing), IDLE).unwrap();
    assert!(row.busy);
}

#[test]
fn expired_bilibili_logins_ask_to_sign_in_while_network_failures_offer_retry() {
    let config = Config::default();
    for error in [
        LoginError::Credentials(CredentialError::Missing),
        LoginError::Credentials(CredentialError::Invalid),
        LoginError::Protocol(BilibiliError::NotAuthenticated),
        LoginError::Protocol(BilibiliError::Api(-101)),
    ] {
        let status = AccountStatus::Failed(error);
        let row = SourceRow::derive(&config, &bilibili(Some(123)), Some(&status), IDLE).unwrap();
        assert_eq!(
            (row.health, row.action),
            (SourceHealth::Problem, Some(SourceAction::SignIn)),
            "{error:?}"
        );
    }
    let status = AccountStatus::Failed(LoginError::Protocol(BilibiliError::Network));
    let row = SourceRow::derive(&config, &bilibili(Some(123)), Some(&status), IDLE).unwrap();
    assert_eq!(
        (row.health, row.action),
        (SourceHealth::Problem, Some(SourceAction::Retry))
    );
}

#[test]
fn a_profile_for_a_missing_server_has_no_row() {
    let profile = SourceProfile::OpenSubsonic {
        server_id: "gone".into(),
    };
    assert!(SourceRow::derive(&Config::default(), &profile, None, IDLE).is_none());
}

#[test]
fn local_folder_rows_report_setup_switch_scanning_and_connected() {
    let config = Config::default();
    let empty = SourceProfile::LocalFolder {
        id: "local-1".into(),
        name: "My FLAC".into(),
        roots: vec![],
    };
    let row = SourceRow::derive(&config, &empty, None, IDLE).unwrap();
    assert_eq!(row.provider, ProviderKind::LocalFolder);
    assert_eq!(row.action, Some(SourceAction::SetUp));

    let populated = SourceProfile::LocalFolder {
        id: "local-1".into(),
        name: "My FLAC".into(),
        roots: vec![std::path::PathBuf::from("/music/flac")],
    };
    let row = SourceRow::derive(&config, &populated, None, IDLE).unwrap();
    assert_eq!(row.action, Some(SourceAction::Switch));
    assert!(row.detail.contains("/music/flac"));

    let connected = Connection {
        current: true,
        loading: false,
        connected: Some("42 tracks · 1 folders".into()),
    };
    let row = SourceRow::derive(&config, &populated, None, connected).unwrap();
    assert_eq!((row.health, row.action), (SourceHealth::Connected, None));
}

#[test]
fn webdav_rows_report_setup_switch_and_connected() {
    let config = Config::default();
    let unconfigured = SourceProfile::WebDav {
        id: "webdav-1".into(),
        name: "NAS".into(),
        base_url: String::new(),
        root_path: "/".into(),
        username: String::new(),
        password: String::new(),
        password_in_keyring: false,
    };
    let row = SourceRow::derive(&config, &unconfigured, None, IDLE).unwrap();
    assert_eq!(row.provider, ProviderKind::WebDav);
    assert_eq!(row.action, Some(SourceAction::SetUp));

    let configured = SourceProfile::WebDav {
        id: "webdav-1".into(),
        name: "NAS".into(),
        base_url: "https://nas.local:5244/dav".into(),
        root_path: "/Music".into(),
        username: "alice".into(),
        password: String::new(),
        password_in_keyring: true,
    };
    let row = SourceRow::derive(&config, &configured, None, IDLE).unwrap();
    assert_eq!(row.action, Some(SourceAction::Switch));
    assert!(row.detail.contains("nas.local/Music"));
}
