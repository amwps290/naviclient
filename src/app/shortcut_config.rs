//! Shortcut-only configuration edits and save-failure rollback.

use crate::models::{Config, GlobalShortcutSettings, ShortcutAction, ShortcutSettings};

pub(super) enum ShortcutConfigEdit {
    LocalBinding(ShortcutAction, String),
    GlobalBinding(ShortcutAction, String),
    LocalDefault(ShortcutAction),
    GlobalDefault(ShortcutAction),
    LocalDefaults,
    GlobalDefaults,
}

enum PreviousBindings {
    Local(ShortcutSettings),
    Global(GlobalShortcutSettings),
}

impl ShortcutConfigEdit {
    pub(super) fn is_global(&self) -> bool {
        matches!(
            self,
            Self::GlobalBinding(..) | Self::GlobalDefault(_) | Self::GlobalDefaults
        )
    }

    /// Validation and conflicts belong to the caller. Persist the new bindings
    /// once, restoring the exact previous map (including missing defaults) on failure.
    pub(super) fn save(
        self,
        config: &mut Config,
        save: impl FnOnce(&Config) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let previous = if self.is_global() {
            PreviousBindings::Global(config.global_shortcuts.clone())
        } else {
            PreviousBindings::Local(config.shortcuts.clone())
        };
        match self {
            Self::LocalBinding(action, binding) => config.shortcuts.set(action, binding),
            Self::GlobalBinding(action, binding) => config.global_shortcuts.set(action, binding),
            Self::LocalDefault(action) => config.shortcuts.reset(action),
            Self::GlobalDefault(action) => config.global_shortcuts.reset(action),
            Self::LocalDefaults => config.shortcuts.reset_all(),
            Self::GlobalDefaults => config.global_shortcuts.reset_all(),
        }
        if let Err(error) = save(config) {
            match previous {
                PreviousBindings::Local(bindings) => config.shortcuts = bindings,
                PreviousBindings::Global(bindings) => config.global_shortcuts = bindings,
            }
            return Err(error);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
