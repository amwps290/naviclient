//! 应用内与系统全局快捷键设置的渲染。

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn render_shortcut_settings(
        &self,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
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
                                section_title(t!("App shortcuts"), cx),
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
                    .pt_6()
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_4()
                            .child(
                                section_title(t!("Global shortcuts"), cx),
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
                    .child(self.render_shortcut_rows(&ShortcutAction::GLOBAL_PLAYBACK, true, cx)),
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
            .child(self.render_shortcut_rows(actions, false, cx))
            .into_any_element()
    }
}
