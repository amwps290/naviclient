//! Transport controls, mode buttons and artwork-based player styles.

use super::artwork_helpers::{accent_foreground, adjust_lightness, readable_accent};
use super::shortcut_bindings::shortcut_display;
use super::*;

impl NavidromeApp {
    pub(super) fn render_player_transport(
        &self,
        playback: &PlaybackState,
        accent: Hsla,
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
                    .custom(play_button_style(accent, cx))
                    .with_size(px(40.0))
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
    pub(super) fn player_toggle_style(
        &self,
        active: bool,
        cx: &Context<Self>,
    ) -> ButtonCustomVariant {
        let accent = self.now_playing_accent(cx);
        if active {
            ButtonCustomVariant::new(cx)
                .color(accent.opacity(0.12))
                .foreground(accent)
                .hover(accent.opacity(0.18))
                .active(accent.opacity(0.24))
        } else {
            ButtonCustomVariant::new(cx)
                .color(cx.theme().transparent)
                .foreground(cx.theme().muted_foreground)
                .hover(accent.opacity(0.08))
                .active(accent.opacity(0.14))
        }
    }

    /// 随机开关按钮：开启时高亮；与循环模式相互独立。
    pub(super) fn shuffle_button(&self, cx: &Context<Self>) -> Button {
        let active = self.state.playback_modes.shuffle;
        Button::new("player-shuffle")
            .icon(AppIcon::Shuffle)
            .tooltip(if active {
                t!("Shuffle on")
            } else {
                t!("Shuffle off")
            })
            .custom(self.player_toggle_style(active, cx))
            .with_size(px(30.0))
            .rounded_full()
            .toggled(active)
            .on_click(cx.listener(|this, _, _, cx| {
                this.toggle_shuffle(cx);
            }))
    }

    /// 循环模式按钮：Off → All → One 循环切换，开启时高亮，
    /// 单曲循环显示专属图标。
    pub(super) fn repeat_button(&self, cx: &Context<Self>) -> Button {
        let repeat = self.state.playback_modes.repeat;
        Button::new("player-repeat")
            .icon(if repeat == RepeatMode::One {
                AppIcon::RepeatOne
            } else {
                AppIcon::Repeat
            })
            .tooltip(repeat.tooltip())
            .custom(self.player_toggle_style(repeat != RepeatMode::Off, cx))
            .with_size(px(30.0))
            .rounded_full()
            .toggled(repeat != RepeatMode::Off)
            .on_click(cx.listener(|this, _, _, cx| {
                this.cycle_repeat(cx);
            }))
    }

    pub(super) fn now_playing_palette(&self, cx: &Context<Self>) -> (Hsla, Hsla) {
        self.state
            .now_playing
            .as_ref()
            .and_then(|song| song.cover_art.as_deref())
            .and_then(|cover_id| self.state.covers.palette(cover_id))
            .unwrap_or((cx.theme().info, cx.theme().chart_2))
    }

    pub(super) fn now_playing_accent(&self, cx: &Context<Self>) -> Hsla {
        let (_, accent) = self.now_playing_palette(cx);
        let accent = if accent.s < 0.16 {
            cx.theme().info
        } else {
            accent
        };
        readable_accent(accent, cx.theme().background)
    }
}

pub(super) fn play_button_style(accent: Hsla, cx: &Context<NavidromeApp>) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .color(accent)
        .foreground(accent_foreground(accent))
        .hover(adjust_lightness(accent, 0.05))
        .active(adjust_lightness(accent, -0.05))
        .shadow(true)
}
