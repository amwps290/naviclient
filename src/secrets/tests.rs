use super::*;
use crate::models::{Config, ServerProfile};

/// 每个测试独享的服务器 id（含进程号），结束后清理凭据库条目。
#[cfg(target_os = "windows")]
fn test_id(name: &str) -> String {
    format!("secrets-test-{}-{name}", std::process::id())
}

#[cfg(target_os = "windows")]
#[test]
fn windows_credential_round_trips_and_deletes() {
    let id = test_id("roundtrip");
    assert!(load_password(&id).is_none(), "unknown entries must miss");

    store_password(&id, "alice", "s3cret-密码").expect("store should succeed");
    assert_eq!(
        load_password(&id),
        Some(("alice".to_string(), "s3cret-密码".to_string()))
    );

    // 覆盖写入：改密码即生效。
    store_password(&id, "alice", "new-pass").expect("overwrite should succeed");
    assert_eq!(
        load_password(&id),
        Some(("alice".to_string(), "new-pass".to_string()))
    );

    assert!(delete_password(&id));
    assert!(load_password(&id).is_none());
    // 再删为 false；未命中读取仍是 None。
    assert!(!delete_password(&id));
}

#[cfg(target_os = "windows")]
#[test]
fn migrate_config_moves_plaintext_passwords_into_the_credential_store() {
    let id = test_id("migrate");
    let mut config = Config {
        servers: vec![ServerProfile {
            id: id.clone(),
            url: "http://nas.local:4533".to_string(),
            username: "alice".to_string(),
            password: "plain-secret".to_string(),
            ..ServerProfile::default()
        }],
        ..Config::default()
    };

    let migrated = migrate_config(&mut config);
    assert_eq!(migrated, vec![id.clone()]);
    let profile = &config.servers[0];
    assert!(profile.password_in_keyring);
    assert!(profile.password.is_empty(), "明文必须被清空");
    assert_eq!(
        load_password(&id).as_ref().map(|(_, p)| p.clone()),
        Some("plain-secret".to_string())
    );
    // 镜像字段同步为空。
    assert!(config.password.is_empty());

    // 已迁移的条目在下一轮启动是 no-op。
    assert!(migrate_config(&mut config).is_empty());

    delete_password(&id);
}

#[cfg(not(target_os = "windows"))]
#[test]
fn migrate_config_keeps_plaintext_when_the_store_is_unavailable() {
    let mut config = Config {
        servers: vec![ServerProfile {
            id: "secrets-test-fallback".to_string(),
            url: "http://nas.local:4533".to_string(),
            username: "alice".to_string(),
            password: "plain-secret".to_string(),
            ..ServerProfile::default()
        }],
        ..Config::default()
    };

    assert!(migrate_config(&mut config).is_empty());
    let profile = &config.servers[0];
    assert!(!profile.password_in_keyring);
    assert_eq!(profile.password, "plain-secret");
}
