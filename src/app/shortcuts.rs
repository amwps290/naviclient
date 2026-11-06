//! 应用内按键分发与系统全局快捷键事件处理。

use super::shortcut_bindings::match_shortcut;
use super::*;

impl NavidromeApp {
    pub(super) fn poll_global_shortcuts(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut changed = false;
        for _ in 0..32 {
            let Some(event) = self.global_shortcuts.try_recv() else {
                break;
            };
            changed = true;
            match event {
                GlobalShortcutEvent::Triggered(action) => {
                    self.execute_playback_shortcut(action, window, cx);
                }
                GlobalShortcutEvent::RegistrationReport { failures } => {
                    self.state.settings.global_shortcut_failures = failures.into_iter().collect();
                }
            }
        }
        changed
    }

    /// 处理窗口内快捷键；录制优先，普通操作遵守输入框、弹窗和菜单焦点边界。
    pub(super) fn handle_keystroke(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let can_record_shortcut =
            self.state.settings.open && self.state.settings.section == SettingsSection::Shortcuts;
        if !can_record_shortcut {
            self.cancel_shortcut_recording();
        }
        if let Some(recording) = self.state.settings.recording() {
            if keystroke.key == "escape" {
                self.cancel_shortcut_recording();
                cx.notify();
                return;
            }
            if !matches!(
                keystroke.key.as_str(),
                "control" | "shift" | "alt" | "platform" | "function"
            ) {
                match recording {
                    ShortcutRecording::Local(action) => self.assign_shortcut(action, keystroke, cx),
                    ShortcutRecording::Global(action) => {
                        self.assign_global_shortcut(action, keystroke, cx)
                    }
                }
            }
            return;
        }
        if window.has_active_dialog(cx)
            || window.has_focused_input(cx)
            || self
                .playlist_ui
                .menu_focus
                .as_ref()
                .is_some_and(|focus| focus.contains_focused(window, cx))
        {
            return;
        }
        let Some(shortcut) = match_shortcut(&self.config.shortcuts, keystroke) else {
            return;
        };
        if !self
            .shortcut_repeat
            .allows(keystroke, shortcut, std::time::Instant::now())
        {
            return;
        }
        match shortcut {
            ShortcutAction::SelectPrevious => {
                self.move_song_selection(-1, cx);
            }
            ShortcutAction::SelectNext => {
                self.move_song_selection(1, cx);
            }
            ShortcutAction::SelectAll => {
                self.select_all_songs(cx);
            }
            ShortcutAction::PlaySelected => {
                self.play_selected_song(cx);
            }
            ShortcutAction::TogglePlayback
            | ShortcutAction::Previous
            | ShortcutAction::Next
            | ShortcutAction::SeekBack
            | ShortcutAction::SeekForward
            | ShortcutAction::VolumeUp
            | ShortcutAction::VolumeDown
            | ShortcutAction::ToggleMute => {
                self.execute_playback_shortcut(shortcut, window, cx);
            }
            ShortcutAction::ToggleNowPlaying => {
                if self.state.view == View::NowPlaying {
                    self.leave_now_playing();
                } else {
                    self.open_now_playing();
                }
                cx.notify();
            }
            ShortcutAction::ToggleQueue => {
                self.state.queue_drawer_open = !self.state.queue_drawer_open;
                cx.notify();
            }
            ShortcutAction::CloseOverlays => {
                self.state.close_top_overlay();
                cx.notify();
            }
            ShortcutAction::FocusSearch => {
                self.focus_search(window, cx);
            }
        }
    }

    pub(super) fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.navigate_to(View::Search);
        cx.notify();
        let mut async_cx = window.to_async(cx);
        let _ = self
            .search_input
            .update_in(&mut async_cx, |input, window, cx| input.focus(window, cx));
    }

    pub(super) fn execute_playback_shortcut(
        &mut self,
        action: ShortcutAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            ShortcutAction::TogglePlayback => self.toggle_playback(),
            ShortcutAction::Previous => self.skip(-1),
            ShortcutAction::Next => self.skip(1),
            ShortcutAction::SeekBack => self.seek_by(-5),
            ShortcutAction::SeekForward => self.seek_by(5),
            ShortcutAction::VolumeUp => self.adjust_volume(0.05, window, cx),
            ShortcutAction::VolumeDown => self.adjust_volume(-0.05, window, cx),
            ShortcutAction::ToggleMute => self.toggle_mute(window, cx),
            _ => return,
        }
        cx.notify();
    }
}
