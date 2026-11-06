//! Player bar layout, current-track information and accessory controls.

use crate::app::playback::display::playback_technical_info;
use crate::app::prelude::*;
use crate::app::shell::view_helpers::compact_window_layout;
use crate::app::shortcuts::bindings::shortcut_display;

impl NavidromeApp {
    pub(in crate::app) fn render_player(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let playback = self.playback_for_display();
        let accent = self.now_playing_accent(cx);
        let viewport_width = f32::from(window.viewport_size().width);
        let compact_player = compact_window_layout(viewport_width);
        // Keep the technical chip on wider windows; details are also available
        // on the now-playing page.
        let show_technical_chip = viewport_width >= 1080.0;
        h_flex()
            .h(px(if compact_player { 82.0 } else { 88.0 }))
            .when(compact_player, |this| this.px_3())
            .when(!compact_player, |this| this.px_4())
            .py_2()
            .gap(if compact_player { px(8.0) } else { px(16.0) })
            .items_center()
            .justify_between()
            .border_t_1()
            .border_color(cx.theme().border.opacity(0.8))
            .bg(cx.theme().secondary.opacity(0.22))
            .child(self.render_player_track(compact_player, cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .gap_1()
                    .items_center()
                    .child(self.render_player_transport(&playback, accent, cx))
                    .child(self.render_player_progress(&playback, accent, cx)),
            )
            .child(self.render_player_actions(compact_player, show_technical_chip, accent, cx))
            .into_any_element()
    }

    fn render_player_track(
        &self,
        compact_player: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let song_info_width = if compact_player { 200.0 } else { 260.0 };
        let player_cover_size = if compact_player { 48.0 } else { 60.0 };
        if let Some(song) = &self.state.now_playing {
            // 悬停提示显示完整标题/艺人（长标题被截断时也能看全文），
            // 并说明点击可打开正在播放页。
            let tooltip_text = if song.artist.trim().is_empty() {
                t!("%{title}\nOpen now playing", title = song.title).to_string()
            } else {
                t!(
                    "%{title} — %{artist}\nOpen now playing",
                    title = song.title,
                    artist = song.artist
                )
                .to_string()
            };
            h_flex()
                .id("now-playing-info")
                .w(px(song_info_width))
                .flex_shrink_0()
                .min_w_0()
                .overflow_hidden()
                .gap(if compact_player { px(8.0) } else { px(12.0) })
                .p_1()
                .items_center()
                .cursor_pointer()
                .rounded(px(12.0))
                .hover(|style| style.bg(cx.theme().accent.opacity(0.08)))
                .tooltip(move |window, cx| Tooltip::new(tooltip_text.clone()).build(window, cx))
                .child(self.render_cover(song.cover_art.as_deref(), player_cover_size, cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .when(compact_player, |this| this.text_sm())
                                .when(!compact_player, |this| this.text_base())
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(song.title.clone()),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(self.song_artist_link(
                                    SharedString::from(format!("player-bar-artist-{}", song.id)),
                                    song,
                                    cx,
                                )),
                        ),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.open_now_playing();
                    cx.notify();
                }))
                .into_any_element()
        } else {
            h_flex()
                .w(px(song_info_width))
                .flex_shrink_0()
                .min_w_0()
                .overflow_hidden()
                .gap(if compact_player { px(8.0) } else { px(12.0) })
                .p_1()
                .items_center()
                .child(self.render_cover(None, player_cover_size, cx))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .when(compact_player, |this| this.text_sm())
                                .when(!compact_player, |this| this.text_base())
                                .font_weight(FontWeight::SEMIBOLD)
                                .truncate()
                                .child(t!("No track playing")),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .truncate()
                                .child(t!("Choose a song from your library.")),
                        ),
                )
                .into_any_element()
        }
    }

    fn render_player_actions(
        &self,
        compact_player: bool,
        show_technical_chip: bool,
        accent: Hsla,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let right_controls_width = if compact_player { 180.0 } else { 260.0 };
        let queue_open = self.state.queue_drawer_open;
        let technical_info = self
            .state
            .now_playing
            .as_ref()
            .zip(self.state.now_playing_quality)
            .map(|(song, quality)| playback_technical_info(song, quality));
        h_flex()
            .w(px(right_controls_width))
            .h(px(40.0))
            .min_w_0()
            .flex_shrink_0()
            .justify_end()
            .items_center()
            .gap(if compact_player { px(4.0) } else { px(8.0) })
            .when(compact_player, |this| this.pl_2())
            .when(!compact_player, |this| this.pl_3())
            .border_l_1()
            .border_color(cx.theme().border.opacity(0.8))
            .when_some(
                technical_info.filter(|_| show_technical_chip),
                |this, info| {
                    let tooltip = info.tooltip.clone();
                    this.child(
                        h_flex()
                            .id("player-technical-info")
                            .min_w_0()
                            .gap_0p5()
                            .tooltip(move |window, cx| {
                                Tooltip::new(tooltip.clone()).build(window, cx)
                            })
                            .child(technical_info_chip(
                                "player-format",
                                info.format,
                                accent,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .h(px(24.0))
                            .border_l_1()
                            .border_color(cx.theme().border.opacity(0.6)),
                    )
                },
            )
            .when_some(self.state.now_playing.as_ref(), |this, song| {
                this.child(
                    self.favorite_button(FavoriteKind::Song, &song.id, cx)
                        .with_size(px(30.0)),
                )
            })
            .child(self.render_eq_popover(cx))
            .child(
                Button::new("player-queue")
                    .icon(AppIcon::Queue)
                    .tooltip(t!(
                        "Queue (%{shortcut})",
                        shortcut = shortcut_display(
                            self.config.shortcuts.binding(ShortcutAction::ToggleQueue)
                        )
                    ))
                    .custom(self.player_toggle_style(queue_open, cx))
                    .with_size(px(30.0))
                    .rounded_full()
                    .toggled(queue_open)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.settings.open = false;
                        this.state.queue_drawer_open = !this.state.queue_drawer_open;
                        if !this.state.queue_drawer_open {
                            this.clear_song_multi();
                        }
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .h(px(24.0))
                    .border_l_1()
                    .border_color(cx.theme().border.opacity(0.6)),
            )
            .child(self.render_volume_control(accent, cx))
            .into_any_element()
    }
}

fn technical_info_chip(
    id: &'static str,
    label: String,
    color: Hsla,
    cx: &Context<NavidromeApp>,
) -> gpui_kit::AnyElement {
    div()
        .id(id)
        .h(px(20.0))
        .max_w(px(64.0))
        .px_1p5()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.0))
        .border_1()
        .border_color(color.opacity(0.12))
        .bg(color.opacity(0.045))
        .text_size(px(12.0))
        .text_color(cx.theme().foreground.opacity(0.74))
        .truncate()
        .child(label)
        .into_any_element()
}
