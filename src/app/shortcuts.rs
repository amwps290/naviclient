//! 应用内快捷键分发、快捷键录制、冲突校验和配置重置。

use super::*;

impl NavidromeApp {
    /// 处理全局键盘快捷键；输入框聚焦时不会触发（由输入框包装层阻止传播）。
    pub(super) fn handle_keystroke(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let can_record_shortcut =
            self.state.settings_open && self.state.settings_section == SettingsSection::Shortcuts;
        if !can_record_shortcut {
            self.cancel_shortcut_recording();
        }
        if let Some(action) = self.state.recording_global_shortcut {
            if keystroke.key == "escape" {
                self.cancel_shortcut_recording();
                cx.notify();
                return;
            }
            if !matches!(
                keystroke.key.as_str(),
                "control" | "shift" | "alt" | "platform" | "function"
            ) {
                self.assign_global_shortcut(action, keystroke, cx);
            }
            return;
        }
        if let Some(action) = self.state.recording_shortcut {
            if keystroke.key == "escape" {
                self.cancel_shortcut_recording();
                cx.notify();
                return;
            }
            if !matches!(
                keystroke.key.as_str(),
                "control" | "shift" | "alt" | "platform" | "function"
            ) {
                self.assign_shortcut(action, keystroke, cx);
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
        // 忽略按住重复触发的同一按键（250ms 内同键视为重复）。
        let now = std::time::Instant::now();
        let pressed_shortcut = keystroke.unparse();
        let repeated = self.last_shortcut == pressed_shortcut
            && now.duration_since(self.last_shortcut_at) < Duration::from_millis(250);
        self.last_shortcut = pressed_shortcut;
        self.last_shortcut_at = now;
        // 选择移动豁免重复保护：按住 Shift+↑/↓ 可连续移动。
        if repeated
            && !matches!(
                shortcut,
                ShortcutAction::SelectPrevious | ShortcutAction::SelectNext
            )
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
                if self.state.song_list_multi.is_some() {
                    // 多选活跃时 Esc 只清空多选，不连带关闭页面/设置。
                    self.clear_song_multi();
                } else if self.state.settings_open {
                    self.state.settings_open = false;
                } else if self.state.queue_drawer_open {
                    self.state.queue_drawer_open = false;
                    self.clear_song_multi();
                } else if self.state.view == View::NowPlaying {
                    self.leave_now_playing();
                } else if let Some(parent) = Self::detail_parent(self.state.view) {
                    self.state.view = parent;
                }
                cx.notify();
            }
            ShortcutAction::FocusSearch => {
                self.focus_search(window, cx);
            }
        }
    }

    pub(super) fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.settings_open = false;
        self.state.view = View::Search;
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

    pub(super) fn start_shortcut_recording(&mut self, action: ShortcutAction, global: bool) {
        self.global_shortcuts.suspend();
        self.state.recording_shortcut = (!global).then_some(action);
        self.state.recording_global_shortcut = global.then_some(action);
        self.state.shortcut_conflict = None;
        self.state.global_shortcut_conflict = None;
        self.state.global_shortcut_validation = None;
    }

    pub(super) fn cancel_shortcut_recording(&mut self) {
        let was_recording = self.state.recording_shortcut.take().is_some();
        let was_recording_global = self.state.recording_global_shortcut.take().is_some();
        self.state.shortcut_conflict = None;
        self.state.global_shortcut_conflict = None;
        self.state.global_shortcut_validation = None;
        if was_recording || was_recording_global {
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
            self.state.shortcut_conflict = Some((action, existing));
            cx.notify();
            return;
        }
        let previous = self.config.shortcuts.clone();
        self.config.shortcuts.set(action, binding);
        if let Err(error) = config::save(&self.config) {
            self.config.shortcuts = previous;
            self.notify_error(t!(
                "Shortcut save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
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
            self.state.global_shortcut_validation = Some((action, error));
            self.state.global_shortcut_conflict = None;
            cx.notify();
            return;
        }
        if let Some(existing) =
            global_shortcut_conflict(&self.config.global_shortcuts, action, &binding)
        {
            self.state.global_shortcut_conflict = Some((action, existing));
            self.state.global_shortcut_validation = None;
            cx.notify();
            return;
        }
        let previous = self.config.global_shortcuts.clone();
        self.config.global_shortcuts.set(action, binding);
        if let Err(error) = config::save(&self.config) {
            self.config.global_shortcuts = previous;
            self.notify_error(t!(
                "Global shortcut save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        self.state.global_shortcut_failures.remove(&action);
        self.cancel_shortcut_recording();
        cx.notify();
    }

    pub(super) fn reset_shortcut(&mut self, action: ShortcutAction, cx: &mut Context<Self>) {
        if let Some(existing) =
            shortcut_conflict(&self.config.shortcuts, action, action.default_binding())
        {
            self.cancel_shortcut_recording();
            self.state.shortcut_conflict = Some((action, existing));
            cx.notify();
            return;
        }
        let previous = self.config.shortcuts.clone();
        self.config.shortcuts.reset(action);
        if let Err(error) = config::save(&self.config) {
            self.config.shortcuts = previous;
            self.notify_error(t!(
                "Shortcut save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        self.cancel_shortcut_recording();
        cx.notify();
    }

    pub(super) fn reset_all_shortcuts(&mut self, cx: &mut Context<Self>) {
        let previous = self.config.shortcuts.clone();
        self.config.shortcuts.reset_all();
        if let Err(error) = config::save(&self.config) {
            self.config.shortcuts = previous;
            self.notify_error(t!(
                "Shortcut save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
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
            self.state.global_shortcut_conflict = Some((action, existing));
            self.state.global_shortcut_validation = None;
            cx.notify();
            return;
        }
        let previous = self.config.global_shortcuts.clone();
        self.config.global_shortcuts.reset(action);
        if let Err(error) = config::save(&self.config) {
            self.config.global_shortcuts = previous;
            self.notify_error(t!(
                "Global shortcut save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        self.state.global_shortcut_failures.remove(&action);
        self.cancel_shortcut_recording();
        self.global_shortcuts.update(&self.config.global_shortcuts);
        cx.notify();
    }

    pub(super) fn reset_all_global_shortcuts(&mut self, cx: &mut Context<Self>) {
        let previous = self.config.global_shortcuts.clone();
        self.config.global_shortcuts.reset_all();
        if let Err(error) = config::save(&self.config) {
            self.config.global_shortcuts = previous;
            self.notify_error(t!(
                "Global shortcut save failed: %{error}",
                error = format!("{error:#}")
            ));
        }
        self.state.global_shortcut_failures.clear();
        self.cancel_shortcut_recording();
        self.global_shortcuts.update(&self.config.global_shortcuts);
        cx.notify();
    }
}
