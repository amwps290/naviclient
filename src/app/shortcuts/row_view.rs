//! Shared binding controls and feedback for app and global shortcut rows.

use super::bindings::{shortcut_display, shortcut_label};
use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn render_shortcut_rows(
        &self,
        actions: &[ShortcutAction],
        global: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        v_flex()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border.opacity(0.72))
            .overflow_hidden()
            .children(
                actions.iter().enumerate().map(|(index, action)| {
                    self.render_shortcut_row(*action, global, index > 0, cx)
                }),
            )
            .into_any_element()
    }

    fn render_shortcut_row(
        &self,
        action: ShortcutAction,
        global: bool,
        separated: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let recording = self.state.settings.is_recording(action, global);
        let (binding, default, conflict) = if global {
            (
                self.config.global_shortcuts.binding(action),
                action
                    .default_global_binding()
                    .expect("global shortcut actions have defaults"),
                self.state.settings.global_shortcut_conflict,
            )
        } else {
            (
                self.config.shortcuts.binding(action),
                action.default_binding(),
                self.state.settings.shortcut_conflict,
            )
        };
        let is_default = binding == default;
        let conflict = conflict
            .filter(|(target, _)| *target == action)
            .map(|(_, existing)| existing);
        let validation = self
            .state
            .settings
            .global_shortcut_validation
            .filter(|(target, _)| global && *target == action)
            .map(|(_, error)| error);
        let registration_failed = global
            && self
                .state
                .settings
                .global_shortcut_failures
                .contains(&action);
        let prefix = if global { "global-" } else { "" };
        let unsupported = global && !cfg!(target_os = "windows");

        v_flex()
            .px_4()
            .py_3()
            .gap_1()
            .when(separated, |row| {
                row.border_t_1().border_color(cx.theme().border)
            })
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_sm()
                            .child(shortcut_label(action)),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                Button::new(SharedString::from(format!(
                                    "{prefix}shortcut-binding-{action:?}"
                                )))
                                .w(px(190.0))
                                .label(if recording {
                                    t!("Press shortcut…").to_string()
                                } else {
                                    shortcut_display(binding)
                                })
                                .outline()
                                .selected(recording)
                                .disabled(unsupported)
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        if this.state.settings.is_recording(action, global) {
                                            this.cancel_shortcut_recording();
                                        } else {
                                            this.start_shortcut_recording(action, global);
                                        }
                                        window.blur(cx);
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                Button::new(SharedString::from(format!(
                                    "{prefix}shortcut-reset-{action:?}"
                                )))
                                .icon(KitIcon::Undo2)
                                .tooltip(t!("Restore default"))
                                .ghost()
                                .small()
                                .disabled(unsupported || is_default)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        if global {
                                            this.reset_global_shortcut(action, cx);
                                        } else {
                                            this.reset_shortcut(action, cx);
                                        }
                                    },
                                )),
                            ),
                    ),
            )
            .when_some(conflict, |row, existing| {
                row.child(div().text_xs().text_color(cx.theme().danger).child(t!(
                    "Already assigned to %{action}",
                    action = shortcut_label(existing)
                )))
            })
            .when_some(validation, |row, error| {
                row.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().danger)
                        .child(match error {
                            GlobalShortcutValidationError::UnsafeModifiers => {
                                t!("Use the Windows key or at least two modifier keys.")
                            }
                            GlobalShortcutValidationError::UnsupportedKey => {
                                t!("That key cannot be used as a global shortcut.")
                            }
                        }),
                )
            })
            .when(registration_failed, |row| {
                row.child(div().text_xs().text_color(cx.theme().danger).child(t!(
                    "Could not register this shortcut. It may already be used by another app."
                )))
            })
            .into_any_element()
    }
}
