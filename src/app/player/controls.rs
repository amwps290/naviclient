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
                Button::new("previous")
                    .icon(PlayerIcon::Previous)
                    .tooltip(t!(
                        "Previous track (%{shortcut})",
                        shortcut = shortcut_display(
                            self.config.shortcuts.binding(ShortcutAction::Previous)
                        )
                    ))
                    .ghost()
                    .with_size(px(30.0))
                    .rounded_full()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.skip(-1);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("play-pause")
                    .icon(if playback.active && !playback.paused {
                        PlayerIcon::Pause
                    } else {
                        PlayerIcon::Play
                    })
                    .tooltip(if playback.active && !playback.paused {
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
                    })
                    .primary()
                    .with_size(px(if compact {
                        tokens::PLAY_BUTTON_COMPACT
                    } else {
                        tokens::PLAY_BUTTON
                    }))
                    .rounded_full()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_playback();
                        cx.notify();
                    })),
            )
            .child(
                Button::new("next")
                    .icon(PlayerIcon::Next)
                    .tooltip(t!(
                        "Next track (%{shortcut})",
                        shortcut =
                            shortcut_display(self.config.shortcuts.binding(ShortcutAction::Next))
                    ))
                    .ghost()
                    .with_size(px(30.0))
                    .rounded_full()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.skip(1);
                        cx.notify();
                    })),
            )
            .child(self.repeat_button(cx))
            .into_any_element()
    }

    /// 播放条模式开关的统一视觉语言：关闭时保持安静，开启时使用当前
    /// 封面强调色。与播放按钮、进度条共用色源，避免默认灰色选中态
    /// 让随机、循环、EQ 和队列看起来像普通悬停。
    pub(in crate::app) fn player_toggle_style(
        &self,
        active: bool,
        cx: &Context<Self>,
    ) -> ButtonCustomVariant {
        soft_toggle_style(active, cx)
    }

    /// 随机开关按钮：开启时高亮；与循环模式相互独立。
    pub(in crate::app) fn shuffle_button(&self, cx: &Context<Self>) -> Button {
        let active = self.state.playback_modes.shuffle;
        Button::new("player-shuffle")
            .icon(AppIcon::Shuffle)
            .tooltip(if active {
                t!("Shuffle on")
            } else {
                t!("Shuffle off")
            })
            .custom(self.player_toggle_style(active, cx))
            .selected(active)
            .with_size(px(30.0))
            .rounded_full()
            .toggled(active)
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_shuffle(cx);
            }))
    }

    /// 循环模式按钮：Off → All → One 循环切换，开启时高亮，
    /// 单曲循环显示专属图标。
    pub(in crate::app) fn repeat_button(&self, cx: &Context<Self>) -> Button {
        let repeat = self.state.playback_modes.repeat;
        Button::new("player-repeat")
            .icon(if repeat == RepeatMode::One {
                AppIcon::RepeatOne
            } else {
                AppIcon::Repeat
            })
            .tooltip(repeat.tooltip())
            .custom(self.player_toggle_style(repeat != RepeatMode::Off, cx))
            .selected(repeat != RepeatMode::Off)
            .with_size(px(30.0))
            .rounded_full()
            .toggled(repeat != RepeatMode::Off)
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
