use super::*;
use crate::models::ServerProfile;

fn config() -> Config {
    Config {
        active_server: Some("active".into()),
        servers: vec![
            ServerProfile {
                id: "active".into(),
                name: "Active server".into(),
                ..ServerProfile::new("https://active.example", "listener", "secret")
            },
            ServerProfile {
                id: "other".into(),
                name: "Other server".into(),
                ..ServerProfile::new("https://other.example", "listener", "secret")
            },
        ],
        ..Default::default()
    }
}

#[test]
fn unknown_targets_never_open_or_submit_a_dialog() {
    let mut state = ServerRemovalState::default();
    assert!(!state.begin(&config(), "missing"));
    assert_eq!(state.label(), None);
    assert_eq!(state.take_target(), None);
    state.cancel();
    assert!(state.begin(&config(), "other"));
}

#[test]
fn duplicate_clicks_cannot_retarget_and_submission_is_single_use() {
    let config = config();
    let original_profiles = config.servers.clone();
    let mut state = ServerRemovalState::default();
    assert!(state.begin(&config, "other"));
    assert!(!state.begin(&config, "other"));
    assert!(!state.begin(&config, "active"));
    assert!(!state.begin(&config, "missing"));
    assert_eq!(state.label(), Some("Other server"));
    assert_eq!(state.take_target().as_deref(), Some("other"));
    assert_eq!(state.label(), None);
    assert_eq!(state.take_target(), None);
    assert_eq!(config.active_server.as_deref(), Some("active"));
    assert_eq!(config.servers, original_profiles);
}

#[test]
fn cancelled_target_cannot_be_submitted_and_later_dialogs_choose_a_new_target() {
    let config = config();
    let mut state = ServerRemovalState::default();
    assert!(state.begin(&config, "other"));
    state.cancel();
    state.cancel();
    assert_eq!(state.label(), None);
    assert_eq!(state.take_target(), None);
    assert!(state.begin(&config, "active"));
    assert_eq!(state.label(), Some("Active server"));
    assert_eq!(state.take_target().as_deref(), Some("active"));
}

#[test]
fn target_identity_and_display_snapshot_survive_profile_changes() {
    let mut config = config();
    let mut state = ServerRemovalState::default();
    assert!(state.begin(&config, "other"));
    config.servers.reverse();
    config.servers[0].name = "Renamed".into();
    assert!(config.switch_to("other"));
    assert!(config.switch_to("active"));
    assert_eq!(state.label(), Some("Other server"));
    assert_eq!(state.take_target().as_deref(), Some("other"));
}

#[test]
fn removed_target_never_falls_back_to_the_active_profile() {
    let mut config = config();
    let mut state = ServerRemovalState::default();
    assert!(state.begin(&config, "other"));
    assert!(config.remove_server("other"));
    let id = state.take_target().unwrap();
    assert_eq!(id, "other");
    assert!(!config.remove_server(&id));
    assert_eq!(config.servers.len(), 1);
    assert_eq!(config.active_server.as_deref(), Some("active"));
    assert_eq!(config.servers[0].id, "active");
    assert_eq!(state.take_target(), None);
}
