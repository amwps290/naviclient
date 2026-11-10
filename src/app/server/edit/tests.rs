use super::*;

use crate::models::{LanguagePreference, ThemePreference};

const URL: &str = "https://music.example";
const USERNAME: &str = "listener";

fn configured(profiles: Vec<ServerProfile>) -> Config {
    let mut config = Config {
        active_server: Some(profiles[0].id.clone()),
        servers: profiles,
        ..Default::default()
    };
    config.sync_active_profile();
    config
}

fn profile_with_id(id: &str, password: &str) -> ServerProfile {
    ServerProfile {
        id: id.into(),
        ..ServerProfile::new(URL, USERNAME, password)
    }
}

#[test]
fn normalization_preserves_password_whitespace() {
    let config = configured(vec![profile_with_id("active", "old")]);
    let edit = ServerForm::new(
        "  https://new.example  ",
        " new-user ",
        "  secret \n",
        " New server \n",
    )
    .prepare(&config, "active", "old")
    .unwrap();
    assert!(edit.requires_reconnect());
    let saved = edit.finish(PasswordStorage::Plaintext);
    let active = saved.active_profile().unwrap();
    assert_eq!(active.url, "https://new.example");
    assert_eq!(active.username, "new-user");
    assert_eq!(active.name, "New server");
    assert_eq!(active.password, "  secret \n");
    assert_eq!(saved.password, active.password);
}

#[test]
fn name_only_active_edit_keeps_its_id_despite_an_equivalent_canonical_profile() {
    let duplicate = ServerProfile::new(URL, USERNAME, "duplicate-password");
    let config = configured(vec![profile_with_id("staging-id", "secret"), duplicate]);
    let duplicate_before = config.servers[1].clone();
    let edit = ServerForm::new(URL, USERNAME, "secret", "New name")
        .prepare(&config, "staging-id", "secret")
        .unwrap();
    assert!(!edit.requires_reconnect());
    let saved = edit.finish(PasswordStorage::Plaintext);
    assert_eq!(saved.active_server_id(), Some("staging-id"));
    assert_eq!(saved.active_profile().unwrap().name, "New name");
    assert_eq!(saved.servers[1], duplicate_before);
}

#[test]
fn active_connection_edit_merges_into_an_existing_canonical_profile() {
    let source = profile_with_id("active", "old");
    let destination = ServerProfile::new("https://other.example", "other-user", "stored");
    let destination_id = destination.id.clone();
    let config = configured(vec![source.clone(), destination]);
    let edit = ServerForm::new("https://other.example", "other-user", "new", "Merged")
        .prepare(&config, "active", "old")
        .unwrap();
    assert!(edit.requires_reconnect());
    assert_eq!(edit.profile().id, destination_id);
    let saved = edit.finish(PasswordStorage::Plaintext);
    assert_eq!(saved.active_server_id(), Some(destination_id.as_str()));
    assert_eq!(saved.servers.len(), 2);
    assert_eq!(saved.servers[0], source);
    assert_eq!(saved.active_profile().unwrap().name, "Merged");
    assert_eq!(saved.server_url, "https://other.example");
    assert_eq!(saved.username, "other-user");
    assert_eq!(saved.password, "new");
}

#[test]
fn inactive_connection_edit_keeps_the_target_and_current_connection() {
    let active = ServerProfile::new(URL, USERNAME, "active-password");
    let config = configured(vec![active.clone(), profile_with_id("inactive", "old")]);
    let edit = ServerForm::new(URL, USERNAME, "new", "Edited inactive")
        .prepare(&config, "inactive", "old")
        .unwrap();
    assert!(!edit.requires_reconnect());
    assert_eq!(edit.profile().id, "inactive");
    let saved = edit.finish(PasswordStorage::Plaintext);
    assert_eq!(saved.active_profile(), Some(&active));
    assert_eq!(saved.password, "active-password");
    assert_eq!(saved.servers[1].password, "new");
    assert_eq!(saved.servers[1].name, "Edited inactive");
}

#[test]
fn button_preview_and_save_decisions_agree_for_active_and_inactive_edits() {
    let config = configured(vec![
        profile_with_id("active", "secret"),
        profile_with_id("inactive", "secret"),
    ]);
    for (url, username, password, identity_changed) in [
        (URL, USERNAME, "secret", false),
        (" https://music.example ", " listener ", "secret", false),
        ("https://new.example", USERNAME, "secret", true),
        (URL, "new-user", "secret", true),
        (URL, USERNAME, "new-password", true),
        (URL, USERNAME, " secret ", true),
    ] {
        for target in &config.servers {
            let form = ServerForm::new(url, username, password, "Renamed");
            let preview = form.requires_reconnect(&config, target, "secret");
            let edit = form.prepare(&config, &target.id, "secret").unwrap();
            let expected = target.id == "active" && identity_changed;
            assert_eq!(preview, expected);
            assert_eq!(edit.requires_reconnect(), expected);
        }
    }
}

#[test]
fn an_unconfigured_active_profile_requires_a_connection_even_without_changes() {
    for (url, username) in [("", ""), (URL, ""), ("", USERNAME)] {
        let config = configured(vec![ServerProfile::new(url, username, "")]);
        let target = config.active_profile().unwrap();
        let form = ServerForm::new(url, username, "", "Name");
        assert!(form.requires_reconnect(&config, target, ""));
        let edit = form.prepare(&config, &target.id, "").unwrap();
        assert!(edit.requires_reconnect());
        assert_eq!(edit.profile().id, target.id);
    }
}

#[test]
fn credential_comparison_uses_the_resolved_password() {
    let mut profile = profile_with_id("active", "");
    profile.password_in_keyring = true;
    let config = configured(vec![profile]);
    let target = config.active_profile().unwrap();
    for (input, reconnect, delete) in [("resolved-secret", false, false), ("", true, true)] {
        let form = ServerForm::new(URL, USERNAME, input, "Name");
        assert_eq!(
            form.requires_reconnect(&config, target, "resolved-secret"),
            reconnect
        );
        let edit = form
            .prepare(&config, &target.id, "resolved-secret")
            .unwrap();
        assert_eq!(edit.requires_reconnect(), reconnect);
        assert_eq!(edit.delete_existing_password(), delete);
    }
}

#[test]
fn clearing_a_password_remembers_the_original_credential_flag_for_either_target() {
    for target_id in ["active", "inactive"] {
        for in_keyring in [false, true] {
            let mut config = configured(vec![
                profile_with_id("active", "secret"),
                profile_with_id("inactive", "secret"),
            ]);
            config
                .servers
                .iter_mut()
                .find(|profile| profile.id == target_id)
                .unwrap()
                .password_in_keyring = in_keyring;
            let edit = ServerForm::new(URL, USERNAME, "", "Name")
                .prepare(&config, target_id, "secret")
                .unwrap();
            assert_eq!(edit.delete_existing_password(), in_keyring);
            let saved = edit.finish(PasswordStorage::Empty);
            let target = saved.servers.iter().find(|p| p.id == target_id).unwrap();
            assert!(target.password.is_empty());
            assert!(!target.password_in_keyring);
            assert_eq!(saved.active_server_id(), Some("active"));
            if target_id == "active" {
                assert!(saved.password.is_empty());
            } else {
                assert_eq!(saved.password, "secret");
            }
        }
    }
}

#[test]
fn clearing_a_merged_password_deletes_the_destination_credential() {
    for destination_in_keyring in [false, true] {
        let mut source = profile_with_id("active", "");
        source.password_in_keyring = !destination_in_keyring;
        let mut destination = ServerProfile::new("https://other.example", USERNAME, "");
        destination.password_in_keyring = destination_in_keyring;
        let destination_id = destination.id.clone();
        let config = configured(vec![source.clone(), destination]);
        let edit = ServerForm::new("https://other.example", USERNAME, "", "Name")
            .prepare(&config, "active", "resolved-source-password")
            .unwrap();
        assert_eq!(edit.profile().id, destination_id);
        assert_eq!(edit.delete_existing_password(), destination_in_keyring);
        let saved = edit.finish(PasswordStorage::Empty);
        assert_eq!(saved.servers[0], source);
        assert_eq!(saved.active_server_id(), Some(destination_id.as_str()));
        assert!(saved.password.is_empty());
        assert!(!saved.active_profile().unwrap().password_in_keyring);
    }
}

#[test]
fn successful_keyring_storage_removes_plaintext_from_the_active_mirror() {
    let config = configured(vec![profile_with_id("active", "old")]);
    let edit = ServerForm::new(URL, USERNAME, "new-secret", "Name")
        .prepare(&config, "active", "old")
        .unwrap();
    assert_eq!(edit.profile().password, "new-secret");
    assert!(!edit.delete_existing_password());
    let saved = edit.finish(PasswordStorage::Keyring);
    let active = saved.active_profile().unwrap();
    assert!(active.password.is_empty());
    assert!(active.password_in_keyring);
    assert!(saved.password.is_empty());
}

#[test]
fn keyring_failure_falls_back_to_the_exact_input_password() {
    let mut active = profile_with_id("active", "");
    active.password_in_keyring = true;
    let config = configured(vec![active]);
    let edit = ServerForm::new(URL, USERNAME, " secret ", "Name")
        .prepare(&config, "active", "resolved-password")
        .unwrap();
    let saved = edit.finish(PasswordStorage::Plaintext);
    assert!(!saved.active_profile().unwrap().password_in_keyring);
    assert_eq!(saved.active_profile().unwrap().password, " secret ");
    assert_eq!(saved.password, " secret ");
}

#[test]
fn storing_an_inactive_password_in_keyring_preserves_the_active_password() {
    let config = configured(vec![
        profile_with_id("active", "active-secret"),
        profile_with_id("inactive", "old"),
    ]);
    let edit = ServerForm::new(URL, USERNAME, "new-secret", "Name")
        .prepare(&config, "inactive", "old")
        .unwrap();
    let saved = edit.finish(PasswordStorage::Keyring);
    assert!(saved.servers[1].password.is_empty());
    assert!(saved.servers[1].password_in_keyring);
    assert_eq!(saved.password, "active-secret");
    assert_eq!(saved.active_profile(), config.active_profile());
}

#[test]
fn a_missing_target_is_rejected_without_changing_the_original_configuration() {
    let config = configured(vec![profile_with_id("active", "secret")]);
    let before = serde_json::to_value(&config).unwrap();
    assert!(ServerForm::new(URL, USERNAME, "changed", "Name")
        .prepare(&config, "missing", "secret")
        .is_none());
    assert_eq!(serde_json::to_value(&config).unwrap(), before);
}

#[test]
fn saving_preserves_other_preferences_and_profiles() {
    let mut config = configured(vec![
        profile_with_id("active", "secret"),
        ServerProfile::new("https://other.example", "other-user", "other-password"),
    ]);
    config.theme = ThemePreference::Dark;
    config.language = LanguagePreference::English;
    config.volume = 0.42;
    config.auto_continue = true;
    config.cache_dir = Some("custom-cache".into());
    config.output_device = Some("custom-device".into());
    config.show_track_notifications = false;
    config.close_minimizes = true;
    let before = serde_json::to_value(&config).unwrap();
    let edit = ServerForm::new(URL, USERNAME, "new-secret", "New name")
        .prepare(&config, "active", "secret")
        .unwrap();
    let saved = edit.finish(PasswordStorage::Keyring);
    assert_eq!(saved.servers[1], config.servers[1]);
    let after = serde_json::to_value(saved).unwrap();
    for (field, expected) in before.as_object().unwrap() {
        if !["servers", "password"].contains(&field.as_str()) {
            assert_eq!(&after[field], expected, "preference {field} changed");
        }
    }
    assert_eq!(serde_json::to_value(&config).unwrap(), before);
}

#[test]
fn a_new_server_exists_only_after_saving_and_then_becomes_the_current_source() {
    let config = configured(vec![profile_with_id("existing", "old")]);
    let browsing = config.active_source.clone();
    assert!(ServerForm::new("", USERNAME, "secret", "")
        .prepare_new(&config)
        .is_none());
    assert!(ServerForm::new(URL, " ", "secret", "")
        .prepare_new(&config)
        .is_none());
    let edit = ServerForm::new("https://new.example", USERNAME, "secret", " Home ")
        .prepare_new(&config)
        .unwrap();
    assert!(edit.requires_reconnect());
    assert_eq!(
        config.servers.len(),
        1,
        "the draft never mutates the original"
    );
    assert_eq!(config.active_source, browsing);
    let saved = edit.finish(PasswordStorage::Plaintext);
    assert_eq!(saved.servers.len(), 2);
    let added = saved.active_profile().unwrap();
    assert_eq!(added.url, "https://new.example");
    assert_eq!(added.name, "Home");
    assert_ne!(added.id, "existing");
    assert!(saved
        .sources
        .iter()
        .any(|profile| profile.id() == saved.active_source.clone().unwrap()));
}

#[test]
fn adding_the_same_server_twice_keeps_both_entries_distinct() {
    let config = configured(vec![ServerProfile::new(URL, USERNAME, "secret")]);
    let saved = ServerForm::new(URL, USERNAME, "secret", "Copy")
        .prepare_new(&config)
        .unwrap()
        .finish(PasswordStorage::Plaintext);
    assert_eq!(saved.servers.len(), 2);
    assert_ne!(saved.servers[0].id, saved.servers[1].id);
    assert_eq!(saved.active_profile().unwrap().name, "Copy");
}
