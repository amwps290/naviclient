//! Transport controls, mode buttons and artwork-based player styles.

use crate::app::prelude::*;
use crate::app::shortcuts::bindings::shortcut_display;
use crate::app::style::cover_tint::{cover_tint, TintSurface};

impl NavidromeApp {
    pub(in crate::app) fn render_player_transport(
        &self,
        playback: &PlaybackState,
        compact: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        h_flex()
            .gap_3()
            .items_center()
            .child(self.shuffle_button(cx))
            .child(
                icon_button(
                    "previous",
                    PlayerIcon::Previous,
                    t!(
                        "Previous track (%{shortcut})",
                        shortcut = shortcut_display(
                            self.config.shortcuts.binding(ShortcutAction::Previous)
                        )
                    ),
                    tokens::ICON_BUTTON_MD,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.skip(-1);
                    cx.notify();
                })),
            )
            .child(
                round_play_button(
                    "play-pause",
                    if playback.active && !playback.paused {
                        PlayerIcon::Pause
                    } else {
                        PlayerIcon::Play
                    },
                    if playback.active && !playback.paused {
                        t!(
                            "Pause (%{shortcut})",
                            shortcut = shortcut_display(
                                self.config
                                    .shortcuts
                                    .binding(ShortcutAction::TogglePlayback)
                            )
                        )
                    } else {
                        t!(
                            "Play (%{shortcut})",
                            shortcut = shortcut_display(
                                self.config
                                    .shortcuts
                                    .binding(ShortcutAction::TogglePlayback)
                            )
                        )
                    },
                    if compact {
                        tokens::PLAY_BUTTON_COMPACT
                    } else {
                        tokens::PLAY_BUTTON
                    },
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.toggle_playback();
                    cx.notify();
                })),
            )
            .child(
                icon_button(
                    "next",
                    PlayerIcon::Next,
                    t!(
                        "Next track (%{shortcut})",
                        shortcut =
                            shortcut_display(self.config.shortcuts.binding(ShortcutAction::Next))
                    ),
                    tokens::ICON_BUTTON_MD,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.skip(1);
                    cx.notify();
                })),
            )
            .child(self.repeat_button(cx))
            .into_any_element()
    }

    /// 随机开关按钮：开启时高亮；与循环模式相互独立。
    pub(in crate::app) fn shuffle_button(&self, cx: &Context<Self>) -> Button {
        let active = self.state.playback_modes.shuffle;
        toggle_icon_button(
            "player-shuffle",
            AppIcon::Shuffle,
            if active {
                t!("Shuffle on")
            } else {
                t!("Shuffle off")
            },
            active,
            tokens::ICON_BUTTON_MD,
            cx,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.toggle_shuffle(cx);
        }))
    }

    /// 循环模式按钮：Off → All → One 循环切换，开启时高亮，
    /// 单曲循环显示专属图标。
    pub(in crate::app) fn repeat_button(&self, cx: &Context<Self>) -> Button {
        let repeat = self.state.playback_modes.repeat;
        let active = repeat != RepeatMode::Off;
        toggle_icon_button(
            "player-repeat",
            if repeat == RepeatMode::One {
                AppIcon::RepeatOne
            } else {
                AppIcon::Repeat
            },
            repeat.tooltip(),
            active,
            tokens::ICON_BUTTON_MD,
            cx,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.cycle_repeat(cx);
        }))
    }

    /// Cover color for a content surface; `None` until the palette has loaded.
    pub(in crate::app) fn tint_for_cover(
        &self,
        surface: TintSurface,
        cover_id: Option<&str>,
        cx: &Context<Self>,
    ) -> Option<(Hsla, Hsla)> {
        let palette = cover_id.and_then(|id| self.browsing_cover_palette(id));
        cover_tint(
            surface,
            palette,
            cx.theme().background,
            cx.theme().is_dark(),
        )
    }

    pub(in crate::app) fn now_playing_cover_palette(&self) -> Option<(Hsla, Hsla)> {
        self.state.now_playing.as_ref().and_then(|song| {
            let source = song.source.as_ref().or_else(|| self.session.source())?;
            self.state
                .covers
                .palette(source, song.cover_art.as_deref()?)
        })
    }
}
