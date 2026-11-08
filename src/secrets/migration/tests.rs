use super::*;
use crate::models::ServerProfile;

fn profile(id: &str) -> ServerProfile {
    ServerProfile {
        id: id.into(),
        name: format!("Server {id}"),
        url: format!("https://{id}.example/music"),
        username: format!("user-{id}"),
        password: format!("password-{id}"),
        password_in_keyring: false,
    }
}

fn config(servers: Vec<ServerProfile>) -> Config {
    Config {
        active_server: servers.first().map(|profile| profile.id.clone()),
        servers,
        server_url: "legacy mirror".into(),
        username: "legacy user".into(),
        password: "legacy password".into(),
        ..Default::default()
    }
}

fn snapshot(config: &Config) -> serde_json::Value {
    serde_json::to_value(config).unwrap()
}

#[test]
fn successful_writes_receive_original_credentials_and_return_ids_in_profile_order() {
    let mut first = profile("first");
    first.username = " 艺人 / & + ".into();
    first.password = "  密码-🔑 & +  ".into();
    let profiles = vec![first, profile("second"), profile("third")];
    let expected_credentials: Vec<_> = profiles
        .iter()
        .map(|profile| {
            (
                profile.id.clone(),
                profile.username.clone(),
                profile.password.clone(),
            )
        })
        .collect();
    let mut config = config(profiles.clone());
    let mut stored = Vec::new();
    let migrated = migrate_config(&mut config, |id, user, password| {
        stored.push((id.to_string(), user.to_string(), password.to_string()));
        Ok(())
    });
    assert_eq!(stored, expected_credentials);
    assert_eq!(migrated, ["first", "second", "third"]);
    for (after, before) in config.servers.iter().zip(profiles) {
        assert_eq!(
            after,
            &ServerProfile {
                password: String::new(),
                password_in_keyring: true,
                ..before
            }
        );
    }
    assert_eq!(config.active_server.as_deref(), Some("first"));
    assert_eq!(config.server_url, "https://first.example/music");
    assert_eq!(config.username, " 艺人 / & + ");
    assert!(config.password.is_empty());
}

#[test]
fn the_first_failed_write_stops_migration_and_retains_that_and_later_profiles() {
    for fail_at in 0..3 {
        let profiles = vec![profile("first"), profile("second"), profile("third")];
        let mut config = config(profiles.clone());
        config.active_server = Some("third".into());
        let before = snapshot(&config);
        let mut attempted = Vec::new();
        let migrated = migrate_config(&mut config, |id, _, _| {
            attempted.push(id.to_string());
            if attempted.len() == fail_at + 1 {
                anyhow::bail!("fixture store unavailable");
            }
            Ok(())
        });
        assert_eq!(
            attempted,
            profiles[..=fail_at]
                .iter()
                .map(|p| p.id.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            migrated,
            profiles[..fail_at]
                .iter()
                .map(|p| p.id.clone())
                .collect::<Vec<_>>()
        );
        for profile in &config.servers[..fail_at] {
            assert!(profile.password.is_empty());
            assert!(profile.password_in_keyring);
        }
        assert_eq!(config.servers[fail_at..], profiles[fail_at..]);
        if fail_at == 0 {
            assert_eq!(snapshot(&config), before);
        } else {
            assert_eq!(config.active_server.as_deref(), Some("third"));
            assert_eq!(config.server_url, "https://third.example/music");
            assert_eq!(config.username, "user-third");
            assert_eq!(config.password, "password-third");
        }
    }
}

#[test]
fn keyring_references_and_empty_passwords_are_skipped_without_changing_profile_metadata() {
    let referenced = ServerProfile {
        password_in_keyring: true,
        ..profile("referenced")
    };
    let empty = ServerProfile {
        password: String::new(),
        ..profile("empty")
    };
    let profiles = vec![referenced.clone(), empty.clone(), profile("eligible")];
    let mut config = config(profiles);
    let mut stored = Vec::new();
    assert_eq!(
        migrate_config(&mut config, |id, user, password| {
            stored.push((id.to_string(), user.to_string(), password.to_string()));
            Ok(())
        }),
        ["eligible"]
    );
    assert_eq!(
        stored,
        [(
            "eligible".into(),
            "user-eligible".into(),
            "password-eligible".into()
        )]
    );
    assert_eq!(config.servers[0], referenced);
    assert_eq!(config.servers[1], empty);
    assert!(config.servers[2].password.is_empty());
    assert!(config.servers[2].password_in_keyring);
}

#[test]
fn no_eligible_profiles_leave_the_entire_config_and_legacy_mirror_unchanged() {
    for profiles in [
        vec![],
        vec![
            ServerProfile {
                password_in_keyring: true,
                ..profile("referenced")
            },
            ServerProfile {
                password: String::new(),
                ..profile("empty")
            },
        ],
    ] {
        let mut config = config(profiles);
        config.active_server = Some("missing".into());
        let before = snapshot(&config);
        assert!(migrate_config(&mut config, |_, _, _| {
            panic!("ineligible profiles must not access the credential store");
        })
        .is_empty());
        assert_eq!(snapshot(&config), before);
    }
}

#[test]
fn partial_success_resynchronizes_the_selected_profile_and_repairs_missing_active_ids() {
    for active in [Some("first"), Some("failed"), Some("missing"), None] {
        let mut config = config(vec![profile("first"), profile("failed")]);
        config.active_server = active.map(str::to_string);
        assert_eq!(
            migrate_config(&mut config, |id, _, _| {
                if id == "failed" {
                    anyhow::bail!("fixture store unavailable");
                }
                Ok(())
            }),
            ["first"]
        );
        let selected = if active == Some("failed") {
            "failed"
        } else {
            "first"
        };
        assert_eq!(config.active_server.as_deref(), Some(selected));
        assert_eq!(
            config.server_url,
            format!("https://{selected}.example/music")
        );
        assert_eq!(config.username, format!("user-{selected}"));
        assert_eq!(
            config.password,
            if selected == "failed" {
                "password-failed"
            } else {
                ""
            }
        );
        assert_eq!(config.servers[1], profile("failed"));
    }
}

#[test]
fn retry_resumes_at_the_failed_profile_and_completed_migrations_never_write_again() {
    let mut config = config(vec![profile("first"), profile("second"), profile("third")]);
    config.active_server = Some("third".into());
    assert_eq!(
        migrate_config(&mut config, |id, _, _| {
            if id == "second" {
                anyhow::bail!("fixture store unavailable");
            }
            Ok(())
        }),
        ["first"]
    );
    let mut retried = Vec::new();
    assert_eq!(
        migrate_config(&mut config, |id, _, _| {
            retried.push(id.to_string());
            Ok(())
        }),
        ["second", "third"]
    );
    assert_eq!(retried, ["second", "third"]);
    assert!(config
        .servers
        .iter()
        .all(|p| p.password.is_empty() && p.password_in_keyring));
    assert_eq!(config.active_server.as_deref(), Some("third"));
    assert_eq!(config.server_url, "https://third.example/music");
    assert_eq!(config.username, "user-third");
    assert!(config.password.is_empty());
    let after_success = snapshot(&config);
    assert!(migrate_config(&mut config, |_, _, _| {
        panic!("completed migrations must not write again");
    })
    .is_empty());
    assert_eq!(snapshot(&config), after_success);
}
#[test]
fn browsing_bilibili_does_not_retain_migrated_server_passwords_in_legacy_mirrors() {
    use crate::source::{ProviderKind, SourceId, SourceProfile};
    let mut config = Config {
        servers: vec![profile("nav-account")],
        ..Config::default()
    };
    config.sync_active_profile();
    config.sources.push(SourceProfile::Bilibili {
        id: "bili-account".into(),
        name: String::new(),
        account_id: None,
        credential_key: Some("bilibili-credential-reference".into()),
        proxy: None,
    });
    let source = SourceId::new(ProviderKind::Bilibili, "bili-account");
    assert!(config.switch_source(&source));
    assert!(!config.password.is_empty());
    let migrated = migrate_config(&mut config, |_, _, _| Ok(()));
    assert_eq!(migrated, vec!["nav-account"]);
    assert!(config.password.is_empty());
    assert!(config.servers[0].password.is_empty());
    assert!(config.servers[0].password_in_keyring);
    assert_eq!(config.active_source, Some(source));
}
