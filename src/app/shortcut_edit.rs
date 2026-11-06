//! Shortcut recording, conflict feedback, persistence, and backend registration.

use super::shortcut_bindings::{global_shortcut_conflict, shortcut_conflict};
use super::shortcut_config::ShortcutConfigEdit;
use super::*;

impl NavidromeApp {
    pub(super) fn start_shortcut_recording(&mut self, action: ShortcutAction, global: bool) {
        self.global_shortcuts.suspend();
        self.state.settings.start_recording(if global {
            ShortcutRecording::Global(action)
        } else {
            ShortcutRecording::Local(action)
        });
    }

    pub(super) fn cancel_shortcut_recording(&mut self) {
        if self.state.settings.cancel_recording() {
            self.global_shortcuts.update(&self.config.global_shortcuts);
        }
    }

    pub(super) fn assign_shortcut(
        &mut self,
        action: ShortcutAction,
        keystroke: &Keystroke,
        cx: &mut Context<Self>,
    ) {
        let binding = keystroke.unparse();
        if let Some(existing) = shortcut_conflict(&self.config.shortcuts, action, &binding) {
            self.state.settings.shortcut_conflict = Some((action, existing));
            cx.notify();
            return;
        }
        self.save_shortcut_edit(ShortcutConfigEdit::LocalBinding(action, binding));
        self.cancel_shortcut_recording();
        cx.notify();
    }

    pub(super) fn assign_global_shortcut(
        &mut self,
        action: ShortcutAction,
        keystroke: &Keystroke,
        cx: &mut Context<Self>,
    ) {
        let binding = keystroke.unparse();
        if let Err(error) = crate::global_shortcuts::validate(&binding) {
            self.state.settings.global_shortcut_validation = Some((action, error));
            self.state.settings.global_shortcut_conflict = None;
            cx.notify();
            return;
        }
        if let Some(existing) =
            global_shortcut_conflict(&self.config.global_shortcuts, action, &binding)
        {
            self.state.settings.global_shortcut_conflict = Some((action, existing));
            self.state.settings.global_shortcut_validation = None;
            cx.notify();
            return;
        }
        self.save_shortcut_edit(ShortcutConfigEdit::GlobalBinding(action, binding));
        self.state.settings.global_shortcut_failures.remove(&action);
        self.cancel_shortcut_recording();
        cx.notify();
    }

    pub(super) fn reset_shortcut(&mut self, action: ShortcutAction, cx: &mut Context<Self>) {
        if let Some(existing) =
            shortcut_conflict(&self.config.shortcuts, action, action.default_binding())
        {
            self.cancel_shortcut_recording();
            self.state.settings.shortcut_conflict = Some((action, existing));
            cx.notify();
            return;
        }
        self.save_shortcut_edit(ShortcutConfigEdit::LocalDefault(action));
        self.cancel_shortcut_recording();
        cx.notify();
    }

    pub(super) fn reset_all_shortcuts(&mut self, cx: &mut Context<Self>) {
        self.save_shortcut_edit(ShortcutConfigEdit::LocalDefaults);
        self.cancel_shortcut_recording();
        cx.notify();
    }

    pub(super) fn reset_global_shortcut(&mut self, action: ShortcutAction, cx: &mut Context<Self>) {
        let default = action
            .default_global_binding()
            .expect("global shortcut actions have defaults");
        if let Some(existing) =
            global_shortcut_conflict(&self.config.global_shortcuts, action, default)
        {
            self.cancel_shortcut_recording();
            self.state.settings.global_shortcut_conflict = Some((action, existing));
            self.state.settings.global_shortcut_validation = None;
            cx.notify();
            return;
        }
        self.save_shortcut_edit(ShortcutConfigEdit::GlobalDefault(action));
        self.state.settings.global_shortcut_failures.remove(&action);
        self.cancel_shortcut_recording();
        self.global_shortcuts.update(&self.config.global_shortcuts);
        cx.notify();
    }

    pub(super) fn reset_all_global_shortcuts(&mut self, cx: &mut Context<Self>) {
        self.save_shortcut_edit(ShortcutConfigEdit::GlobalDefaults);
        self.state.settings.global_shortcut_failures.clear();
        self.cancel_shortcut_recording();
        self.global_shortcuts.update(&self.config.global_shortcuts);
        cx.notify();
    }

    fn save_shortcut_edit(&mut self, edit: ShortcutConfigEdit) {
        let global = edit.is_global();
        if let Err(error) = edit.save(&mut self.config, config::save) {
            self.notify_error(if global {
                t!(
                    "Global shortcut save failed: %{error}",
                    error = format!("{error:#}")
                )
            } else {
                t!(
                    "Shortcut save failed: %{error}",
                    error = format!("{error:#}")
                )
            });
        }
    }
}
