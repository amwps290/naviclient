//! Registration replacement, failure ordering and ownership, separate from the OS API.

use std::collections::HashMap;

use crate::models::{GlobalShortcutSettings, ShortcutAction};

use super::binding::Hotkey;

const FIRST_ID: i32 = 0x4e00;

pub(super) fn snapshot(settings: &GlobalShortcutSettings) -> Vec<(ShortcutAction, String)> {
    ShortcutAction::GLOBAL_PLAYBACK
        .into_iter()
        .map(|action| (action, settings.binding(action).to_string()))
        .collect()
}

pub(super) trait HotkeySystem {
    fn register(&mut self, id: i32, hotkey: Hotkey) -> bool;
    fn unregister(&mut self, id: i32);
}

pub(super) struct Registrations<B: HotkeySystem> {
    system: B,
    actions: HashMap<i32, ShortcutAction>,
}

impl<B: HotkeySystem> Registrations<B> {
    pub(super) fn new(system: B) -> Self {
        Self {
            system,
            actions: HashMap::new(),
        }
    }

    pub(super) fn replace(
        &mut self,
        bindings: Vec<(ShortcutAction, String)>,
    ) -> Vec<ShortcutAction> {
        self.clear();
        let mut failures = Vec::new();
        for (index, (action, binding)) in bindings.into_iter().enumerate() {
            let Ok(hotkey) = Hotkey::parse(&binding) else {
                failures.push(action);
                continue;
            };
            // Invalid or failed registrations retain their original slot; IDs
            // for later bindings must not shift when an earlier one fails.
            let id = FIRST_ID + index as i32;
            if self.system.register(id, hotkey) {
                self.actions.insert(id, action);
            } else {
                failures.push(action);
            }
        }
        failures
    }

    pub(super) fn action(&self, id: i32) -> Option<ShortcutAction> {
        self.actions.get(&id).copied()
    }

    pub(super) fn clear(&mut self) {
        for id in self.actions.keys().copied().collect::<Vec<_>>() {
            self.system.unregister(id);
        }
        self.actions.clear();
    }
}

impl<B: HotkeySystem> Drop for Registrations<B> {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests;
