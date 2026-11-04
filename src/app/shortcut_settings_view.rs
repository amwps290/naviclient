//! 应用内与系统全局快捷键设置的渲染。

use super::*;

impl NavidromeApp {
    pub(super) fn render_shortcut_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        const PLAYBACK: &[ShortcutAction] = &[
            ShortcutAction::TogglePlayback,
            ShortcutAction::Previous,
            ShortcutAction::Next,
            ShortcutAction::SeekBack,
            ShortcutAction::SeekForward,
            ShortcutAction::VolumeUp,
            ShortcutAction::VolumeDown,
            ShortcutAction::ToggleMute,
        ];
        const NAVIGATION: &[ShortcutAction] = &[
            ShortcutAction::ToggleNowPlaying,
            ShortcutAction::ToggleQueue,
            ShortcutAction::FocusSearch,
            ShortcutAction::CloseOverlays,
        ];
        const SONG_LIST: &[ShortcutAction] = &[
            ShortcutAction::SelectPrevious,
            ShortcutAction::SelectNext,
            ShortcutAction::SelectAll,
            ShortcutAction::PlaySelected,
        ];
        let all_default = ShortcutAction::ALL
            .into_iter()
            .all(|action| self.config.shortcuts.binding(action) == action.default_binding());
        let all_global_default = ShortcutAction::GLOBAL_PLAYBACK.into_iter().all(|action| {
            self.config.global_shortcuts.binding(action)
                == action
                    .default_global_binding()
                    .expect("global shortcut actions have defaults")
        });

        v_flex()
            .gap_6()
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_4()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(t!("App shortcuts")),
                            )
                            .child(
                                Button::new("reset-all-shortcuts")
                                    .label(t!("Restore app defaults"))
                                    .ghost()
                                    .small()
                                    .disabled(all_default)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.reset_all_shortcuts(cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(t!(
                                "These shortcuts work while the app window is active. Click a shortcut, then press a new combination; press Esc to cancel."
                            )),
                    )
                    .child(self.render_shortcut_group(t!("Playback controls"), PLAYBACK, cx))
                    .child(self.render_shortcut_group(t!("Navigation"), NAVIGATION, cx))
                    .child(self.render_shortcut_group(t!("Song list"), SONG_LIST, cx)),
            )
            .child(
                v_flex()
                    .gap_3()
                    .pt_5()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_4()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(t!("Global shortcuts")),
                            )
                            .child(
                                Button::new("reset-all-global-shortcuts")
                                    .label(t!("Restore global defaults"))
                                    .ghost()
                                    .small()
                                    .disabled(!cfg!(target_os = "windows") || all_global_default)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.reset_all_global_shortcuts(cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(if cfg!(target_os = "windows") {
                                t!("Control playback while the app is hidden or another window is active. Use the Windows key or at least two modifier keys.")
                            } else {
                                t!("Custom global shortcuts are currently available on Windows.")
                            }),
                    )
                    .child(self.render_global_shortcut_group(cx)),
            )
            .into_any_element()
    }

    fn render_shortcut_group(
        &self,
        title: impl Into<SharedString>,
        actions: &'static [ShortcutAction],
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        v_flex()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(title.into()),
            )
            .child(
                v_flex()
                    .rounded_lg()
                    .border_1()
                    .border_color(cx.theme().border.opacity(0.72))
                    .overflow_hidden()
                    .children(
                        actions.iter().enumerate().map(|(index, action)| {
                            self.render_shortcut_row(*action, index > 0, cx)
                        }),
                    ),
            )
            .into_any_element()
    }

    fn render_shortcut_row(
        &self,
        action: ShortcutAction,
        separated: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let recording = self.state.recording_shortcut == Some(action);
        let binding = self.config.shortcuts.binding(action);
        let is_default = binding == action.default_binding();
        let conflict = self
            .state
            .shortcut_conflict
            .filter(|(target, _)| *target == action)
            .map(|(_, existing)| existing);

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
                                    "shortcut-binding-{action:?}"
                                )))
                                .w(px(190.0))
                                .label(if recording {
                                    t!("Press shortcut…").to_string()
                                } else {
                                    shortcut_display(binding)
                                })
                                .outline()
                                .selected(recording)
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        if this.state.recording_shortcut == Some(action) {
                                            this.cancel_shortcut_recording();
                                        } else {
                                            this.start_shortcut_recording(action, false);
                                        }
                                        window.blur(cx);
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                Button::new(SharedString::from(format!(
                                    "shortcut-reset-{action:?}"
                                )))
                                .icon(KitIcon::Undo2)
                                .tooltip(t!("Restore default"))
                                .ghost()
                                .small()
                                .disabled(is_default)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.reset_shortcut(action, cx);
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
            .into_any_element()
    }

    fn render_global_shortcut_group(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border.opacity(0.72))
            .overflow_hidden()
            .children(
                ShortcutAction::GLOBAL_PLAYBACK
                    .iter()
                    .enumerate()
                    .map(|(index, action)| self.render_global_shortcut_row(*action, index > 0, cx)),
            )
            .into_any_element()
    }

    fn render_global_shortcut_row(
        &self,
        action: ShortcutAction,
        separated: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let recording = self.state.recording_global_shortcut == Some(action);
        let binding = self.config.global_shortcuts.binding(action);
        let is_default = binding
            == action
                .default_global_binding()
                .expect("global shortcut actions have defaults");
        let conflict = self
            .state
            .global_shortcut_conflict
            .filter(|(target, _)| *target == action)
            .map(|(_, existing)| existing);
        let validation = self
            .state
            .global_shortcut_validation
            .filter(|(target, _)| *target == action)
            .map(|(_, error)| error);
        let registration_failed = self.state.global_shortcut_failures.contains(&action);

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
                                    "global-shortcut-binding-{action:?}"
                                )))
                                .w(px(190.0))
                                .label(if recording {
                                    t!("Press shortcut…").to_string()
                                } else {
                                    shortcut_display(binding)
                                })
                                .outline()
                                .selected(recording)
                                .disabled(!cfg!(target_os = "windows"))
                                .on_click(cx.listener(
                                    move |this, _, window, cx| {
                                        if this.state.recording_global_shortcut == Some(action) {
                                            this.cancel_shortcut_recording();
                                        } else {
                                            this.start_shortcut_recording(action, true);
                                        }
                                        window.blur(cx);
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                Button::new(SharedString::from(format!(
                                    "global-shortcut-reset-{action:?}"
                                )))
                                .icon(KitIcon::Undo2)
                                .tooltip(t!("Restore default"))
                                .ghost()
                                .small()
                                .disabled(!cfg!(target_os = "windows") || is_default)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.reset_global_shortcut(action, cx);
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
