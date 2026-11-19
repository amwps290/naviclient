//! Applying fullscreen intent without replacing playback or library state.

use std::time::Instant;

use gpui_kit::{Bounds, FocusHandle, Size};

use super::layout::FullscreenLayout;
use super::state::FullscreenState;
use crate::app::prelude::*;
use crate::audio::visualizer::WAVEFORM_BINS;

pub(in crate::app) struct FullscreenUi {
    pub(super) state: FullscreenState,
    pub(super) scroll: ScrollHandle,
    pub(super) focus: FocusHandle,
    pub(super) header_focus: FocusHandle,
    pub(super) footer_focus: FocusHandle,
    pub(super) translated: bool,
    pub(super) waveform: [f32; WAVEFORM_BINS],
    pub(super) pointer_down: bool,
    previous_focus: Option<FocusHandle>,
    viewport: Option<Size<Pixels>>,
    lyric_bounds: Option<(usize, Bounds<Pixels>, Bounds<Pixels>)>,
    pointer: Option<gpui_kit::Point<Pixels>>,
}

impl FullscreenUi {
    pub(in crate::app) fn new(cx: &App) -> Self {
        Self {
            state: FullscreenState::default(),
            scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            header_focus: cx.focus_handle(),
            footer_focus: cx.focus_handle(),
            translated: true,
            waveform: [0.0; WAVEFORM_BINS],
            pointer_down: false,
            previous_focus: None,
            viewport: None,
            lyric_bounds: None,
            pointer: None,
        }
    }

    pub(in crate::app) fn is_active(&self) -> bool {
        self.state.active()
    }

    pub(in crate::app) fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll
    }
}

impl NavidromeApp {
    pub(in crate::app) fn toggle_fullscreen_player(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.fullscreen.is_active() {
            self.exit_fullscreen_player(window, cx);
            return;
        }
        if self.mini_window.is_mini() {
            self.exit_mini_mode(window, cx);
        }
        self.state.settings.close();
        self.state.queue_drawer_open = false;
        self.lyrics_ui.cancel();
        self.fullscreen.previous_focus = window.focused(cx);
        self.fullscreen.translated = self.config.show_lyrics_translation;
        self.fullscreen.viewport = None;
        self.fullscreen.lyric_bounds = None;
        self.fullscreen.pointer_down = false;
        self.fullscreen.pointer = Some(window.mouse_position());
        self.fullscreen.waveform.fill(0.0);
        self.state.lyrics.follow_resume = true;
        self.state.lyrics.scroll_target = None;
        if self
            .fullscreen
            .state
            .enter(window.is_fullscreen(), Instant::now())
        {
            window.toggle_fullscreen();
        }
        window.focus(&self.fullscreen.focus, cx);
        cx.notify();
    }

    pub(in crate::app) fn exit_fullscreen_player(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.fullscreen.is_active() {
            return;
        }
        if self
            .fullscreen
            .state
            .exit(window.is_fullscreen(), Instant::now())
        {
            window.toggle_fullscreen();
        }
        self.finish_fullscreen_exit(window, cx);
        cx.notify();
    }

    fn finish_fullscreen_exit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.fullscreen.pointer_down = false;
        self.fullscreen.lyric_bounds = None;
        self.fullscreen.waveform.fill(0.0);
        self.audio.set_visualizer_enabled(false);
        self.state.lyrics.scroll_target = None;
        self.state.lyrics.follow_resume = true;
        if let Some(focus) = self.fullscreen.previous_focus.take() {
            window.focus(&focus, cx);
        } else {
            window.blur(cx);
        }
    }

    pub(in crate::app) fn sync_fullscreen_viewport(&mut self, window: &Window) {
        let viewport = window.viewport_size();
        if self.fullscreen.is_active() && self.fullscreen.viewport != Some(viewport) {
            self.fullscreen.viewport = Some(viewport);
            self.fullscreen.lyric_bounds = None;
            self.state.lyrics.scroll_target = None;
            self.state.lyrics.follow_resume = true;
        }
    }

    pub(in crate::app) fn poll_fullscreen(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let now = Instant::now();
        if !window.is_window_active() {
            self.fullscreen.pointer_down = false;
        }
        let playback = self.playback_for_display();
        let layout = FullscreenLayout::new(
            f32::from(window.viewport_size().width),
            f32::from(window.viewport_size().height),
        );
        let pointer_y = f32::from(window.mouse_position().y);
        let over_controls = self.fullscreen.state.opacity() > 0.0
            && (pointer_y <= layout.padding_top + layout.header_height
                || pointer_y
                    >= f32::from(window.viewport_size().height)
                        - layout.padding_bottom
                        - layout.footer_height);
        let keep_controls = over_controls
            || self.fullscreen.pointer_down
            || self.fullscreen.focus.contains_focused(window, cx)
                && !self.fullscreen.focus.is_focused(window)
            || self.fullscreen.header_focus.contains_focused(window, cx)
            || self.fullscreen.footer_focus.contains_focused(window, cx)
            || playback.paused
            || !playback.active
            || playback.stall.is_some()
            || playback.error.is_some();
        let update = self
            .fullscreen
            .state
            .poll(window.is_fullscreen(), now, keep_controls);
        if update.toggle_native {
            window.toggle_fullscreen();
        }
        if update.exited {
            self.finish_fullscreen_exit(window, cx);
        }
        let visible = window.is_visible() && self.fullscreen.is_active();
        self.audio.set_visualizer_enabled(
            visible
                && self.fullscreen.state.visualizer_enabled
                && playback.active
                && !playback.paused
                && playback.output_connected
                && playback.stall.is_none()
                && playback.error.is_none(),
        );
        let target = self.audio.waveform();
        let mut changed = update.changed;
        for (value, target) in self.fullscreen.waveform.iter_mut().zip(target) {
            let next = *value + (target - *value) * 0.35;
            changed |= visible && (next - *value).abs() > 0.001;
            *value = if next.abs() < 0.001 { 0.0 } else { next };
        }
        if self.fullscreen.is_active() {
            self.recenter_fullscreen_lyrics(&playback);
        }
        changed
    }

    fn recenter_fullscreen_lyrics(&mut self, playback: &PlaybackState) {
        let active = self
            .state
            .lyrics
            .lyrics()
            .filter(|lyrics| lyrics.is_synced())
            .and_then(|lyrics| lyrics.display_line_index(playback.position));
        let bounds = active.and_then(|index| {
            self.fullscreen
                .scroll
                .bounds_for_item(index + 1)
                .map(|bounds| (index, bounds, self.fullscreen.scroll.bounds()))
        });
        if bounds != self.fullscreen.lyric_bounds {
            self.fullscreen.lyric_bounds = bounds;
            self.state.lyrics.follow_resume = true;
        }
    }

    pub(super) fn wake_fullscreen_controls(&mut self, cx: &mut Context<Self>) {
        if self.fullscreen.state.wake(Instant::now()) {
            cx.notify();
        }
    }

    pub(super) fn move_fullscreen_pointer(
        &mut self,
        position: gpui_kit::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        // GPUI also sends a stationary move when redraws update hitboxes.
        if self.fullscreen.pointer != Some(position) {
            self.fullscreen.pointer = Some(position);
            self.wake_fullscreen_controls(cx);
        }
    }

    pub(super) fn toggle_fullscreen_controls(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.fullscreen.state.toggle_controls(Instant::now());
        window.focus(&self.fullscreen.focus, cx);
        cx.notify();
    }

    pub(super) fn toggle_fullscreen_translation(&mut self, cx: &mut Context<Self>) {
        self.fullscreen.translated = !self.fullscreen.translated;
        self.fullscreen.lyric_bounds = None;
        self.state.lyrics.follow_resume = true;
        self.wake_fullscreen_controls(cx);
        cx.notify();
    }

    pub(super) fn toggle_fullscreen_visualizer(&mut self, cx: &mut Context<Self>) {
        self.fullscreen.state.visualizer_enabled = !self.fullscreen.state.visualizer_enabled;
        self.wake_fullscreen_controls(cx);
        cx.notify();
    }

    pub(in crate::app) fn handle_fullscreen_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.fullscreen.is_active() {
            return false;
        }
        self.wake_fullscreen_controls(cx);
        let plain = keystroke.modifiers == gpui_kit::Modifiers::default();
        if plain && keystroke.key == "escape" {
            self.exit_fullscreen_player(window, cx);
            return true;
        }
        // User bindings retain precedence over the presentation's extra keys.
        if crate::app::shortcuts::bindings::match_shortcut(&self.config.shortcuts, keystroke)
            .is_some()
            || !plain
        {
            return false;
        }
        if !matches!(keystroke.key.as_str(), "h" | "t" | "v" | "f") {
            return false;
        }
        if self
            .shortcut_repeat
            .allows(keystroke, ShortcutAction::ToggleFullscreen, Instant::now())
        {
            match keystroke.key.as_str() {
                "h" => self.toggle_fullscreen_controls(window, cx),
                "t" => self.toggle_fullscreen_translation(cx),
                "v" => self.toggle_fullscreen_visualizer(cx),
                "f" => self.exit_fullscreen_player(window, cx),
                _ => unreachable!(),
            }
        }
        true
    }
}
