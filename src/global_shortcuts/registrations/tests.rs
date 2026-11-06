use super::*;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

#[derive(Debug, PartialEq, Eq)]
enum Call {
    Register(i32, Hotkey),
    Unregister(i32),
}

#[derive(Default)]
struct State {
    active: HashMap<i32, Hotkey>,
    fail_ids: HashSet<i32>,
    panic_id: Option<i32>,
    calls: Vec<Call>,
}

struct FakeSystem(Arc<Mutex<State>>);

impl HotkeySystem for FakeSystem {
    fn register(&mut self, id: i32, hotkey: Hotkey) -> bool {
        let mut state = self.0.lock().unwrap();
        state.calls.push(Call::Register(id, hotkey));
        if state.panic_id == Some(id) {
            drop(state);
            panic!("synthetic registration panic");
        }
        if state.fail_ids.contains(&id) {
            return false;
        }
        assert!(state.active.insert(id, hotkey).is_none());
        true
    }

    fn unregister(&mut self, id: i32) {
        let mut state = self.0.lock().unwrap();
        state.calls.push(Call::Unregister(id));
        assert!(state.active.remove(&id).is_some());
    }
}

fn registrations() -> (Registrations<FakeSystem>, Arc<Mutex<State>>) {
    let state = Arc::new(Mutex::new(State::default()));
    (Registrations::new(FakeSystem(state.clone())), state)
}

fn defaults() -> Vec<(ShortcutAction, String)> {
    snapshot(&GlobalShortcutSettings::default())
}

#[test]
fn snapshots_keep_eight_action_slots_custom_bindings_defaults_and_owned_data() {
    let mut settings = GlobalShortcutSettings::default();
    settings.bindings.remove(&ShortcutAction::Previous);
    settings.set(ShortcutAction::Next, "win-f24".into());
    settings
        .bindings
        .insert(ShortcutAction::FocusSearch, "win-f".into());
    let prepared = snapshot(&settings);
    settings.reset_all();
    assert_eq!(
        prepared
            .iter()
            .map(|(action, _)| *action)
            .collect::<Vec<_>>(),
        ShortcutAction::GLOBAL_PLAYBACK
    );
    assert_eq!(prepared[1].1, "ctrl-alt-left");
    assert_eq!(prepared[2].1, "win-f24");
    assert!(prepared
        .iter()
        .all(|(_, binding)| Hotkey::parse(binding).is_ok()));
}

#[test]
fn mixed_validation_and_native_failures_keep_ids_and_report_order_without_registering_invalid_keys()
{
    let (mut registrations, state) = registrations();
    state.lock().unwrap().fail_ids.insert(FIRST_ID + 2);
    let mut bindings = defaults();
    bindings[1].1 = "space".into();
    bindings[4].1 = "ctrl-alt-🙂".into();
    assert_eq!(
        registrations.replace(bindings),
        [
            ShortcutAction::Previous,
            ShortcutAction::Next,
            ShortcutAction::SeekForward
        ]
    );
    let state = state.lock().unwrap();
    assert_eq!(
        state
            .calls
            .iter()
            .filter_map(|call| match call {
                Call::Register(id, _) => Some(*id),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [
            FIRST_ID,
            FIRST_ID + 2,
            FIRST_ID + 3,
            FIRST_ID + 5,
            FIRST_ID + 6,
            FIRST_ID + 7
        ]
    );
    assert_eq!(state.active.len(), 5);
    for (index, action) in ShortcutAction::GLOBAL_PLAYBACK.into_iter().enumerate() {
        let expected = if [1, 2, 4].contains(&index) {
            None
        } else {
            Some(action)
        };
        assert_eq!(registrations.action(FIRST_ID + index as i32), expected);
    }
    assert!(registrations.action(FIRST_ID - 1).is_none());
    assert!(registrations.action(FIRST_ID + 8).is_none());
}

#[test]
fn replacement_unregisters_all_owned_keys_before_any_new_attempt_and_updates_event_targets() {
    let (mut registrations, state) = registrations();
    assert!(registrations.replace(defaults()).is_empty());
    state.lock().unwrap().calls.clear();
    let bindings = vec![
        (ShortcutAction::Next, "win-n".into()),
        (ShortcutAction::Previous, "invalid".into()),
    ];
    assert_eq!(registrations.replace(bindings), [ShortcutAction::Previous]);
    let state = state.lock().unwrap();
    assert_eq!(state.calls.len(), 9);
    assert_eq!(
        state.calls[..8]
            .iter()
            .map(|call| match call {
                Call::Unregister(id) => *id,
                _ => panic!("new registration preceded cleanup"),
            })
            .collect::<HashSet<_>>(),
        (FIRST_ID..FIRST_ID + 8).collect()
    );
    assert_eq!(
        state.calls[8],
        Call::Register(FIRST_ID, Hotkey::parse("win-n").unwrap())
    );
    assert_eq!(registrations.action(FIRST_ID), Some(ShortcutAction::Next));
    assert_eq!(registrations.action(FIRST_ID + 1), None);
    assert_eq!(state.active.len(), 1);
}

#[test]
fn suspension_and_empty_or_failed_replacement_release_keys_once_and_can_be_resumed() {
    let (mut registrations, state) = registrations();
    assert!(registrations.replace(defaults()).is_empty());
    registrations.clear();
    registrations.clear();
    assert!((FIRST_ID..FIRST_ID + 8).all(|id| registrations.action(id).is_none()));
    assert!(registrations.replace(defaults()).is_empty());
    assert!(registrations.replace(vec![]).is_empty());
    assert!(registrations.replace(defaults()).is_empty());
    state
        .lock()
        .unwrap()
        .fail_ids
        .extend(FIRST_ID..FIRST_ID + 8);
    assert_eq!(
        registrations.replace(defaults()),
        ShortcutAction::GLOBAL_PLAYBACK
    );
    let count = state.lock().unwrap().calls.len();
    assert!(state.lock().unwrap().active.is_empty());
    drop(registrations);
    assert_eq!(state.lock().unwrap().calls.len(), count);
}

#[test]
fn dropping_the_owner_releases_only_successfully_registered_keys() {
    let (mut registrations, state) = registrations();
    state
        .lock()
        .unwrap()
        .fail_ids
        .extend([FIRST_ID, FIRST_ID + 7]);
    assert_eq!(
        registrations.replace(defaults()),
        [ShortcutAction::TogglePlayback, ShortcutAction::ToggleMute]
    );
    state.lock().unwrap().calls.clear();
    drop(registrations);
    let state = state.lock().unwrap();
    assert!(state.active.is_empty());
    assert_eq!(state.calls.len(), 6);
    assert!(state.calls.iter().all(
        |call| matches!(call, Call::Unregister(id) if *id != FIRST_ID && *id != FIRST_ID + 7)
    ));
}

#[test]
fn worker_unwind_releases_keys_acquired_before_a_registration_panic() {
    let state = Arc::new(Mutex::new(State {
        panic_id: Some(FIRST_ID + 1),
        ..Default::default()
    }));
    let system = FakeSystem(state.clone());
    let result = std::panic::catch_unwind(move || {
        let mut registrations = Registrations::new(system);
        registrations.replace(defaults());
    });
    assert!(result.is_err());
    let state = state.lock().unwrap();
    assert!(state.active.is_empty());
    assert_eq!(
        state.calls,
        [
            Call::Register(FIRST_ID, Hotkey::parse("ctrl-alt-space").unwrap()),
            Call::Register(FIRST_ID + 1, Hotkey::parse("ctrl-alt-left").unwrap()),
            Call::Unregister(FIRST_ID),
        ]
    );
}
