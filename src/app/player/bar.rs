//! Player bar layout, current-track information and accessory controls.

use crate::app::playback::display::playback_technical_info;
use crate::app::prelude::*;
use crate::app::shortcuts::bindings::shortcut_display;

impl NavidromeApp {
    pub(in crate::app) fn render_player(
        &self,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let playback = self.playback_for_display();
        let brand = palette(cx).brand;
        let viewport_width = f32::from(window.viewport_size().width);
        let layout = self.shell_layout(window);
        let compact_player = layout.compact;
        // Keep the technical chip on wider windows; details are also available
        // on the now-playing page.
        let show_technical_chip = viewport_width >= 1080.0;
        raised_surface(
            h_flex()
                .flex_none()
                .h(px(if compact_player {
                    tokens::PLAYER_COMPACT_HEIGHT
                } else {
                    tokens::PLAYER_HEIGHT
                }))
                .mx(px(layout.player_margin))
                .mb(px(layout.player_margin))
                .when(compact_player, |this| this.px_2())
                .when(!compact_player, |this| this.px_3())
                .gap(if compact_player { px(8.0) } else { px(16.0) })
                .items_center()
                .justify_between(),
            tokens::radius::LARGE,
            cx,
        )
        .child(self.render_player_track(compact_player, cx))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .items_center()
                .child(self.render_player_transport(&playback, compact_player, cx))
                .child(self.render_player_progress(&playback, brand, cx)),
        )
        .child(self.render_player_actions(compact_player, show_technical_chip, brand, cx))
        .into_any_element()
    }

    fn render_player_track(
        &self,
        compact_player: bool,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let song_info_width = if compact_player { 200.0 } else { 260.0 };
        let player_cover_size = if compact_player { 40.0 } else { 48.0 };
        if let Some(song) = &self.state.now_playing {
            // 悬停提示显示完整标题/艺人（长标题被截断时也能看全文），
            // 并说明点击可打开正在播放页。
            let tooltip_title = song
                .bilibili_subtitle()
                .map(|subtitle| format!("{}\n{subtitle}", song.title))
                .unwrap_or_else(|| song.title.clone());
            let tooltip_artist = song
                .bilibili_part
                .as_ref()
                .map(|part| t!("Uploader: %{name}", name = part.uploader).to_string())
                .unwrap_or_else(|| song.artist.clone());
            let tooltip_text = if tooltip_artist.trim().is_empty() {
                t!("%{title}\nOpen now playing", title = tooltip_title).to_string()
            } else {
                t!(
                    "%{title} — %{artist}\nOpen now playing",
                    title = tooltip_title,
                    artist = tooltip_artist
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
                .rounded(px(tokens::radius::COVER))
                .hover(|style| style.bg(palette(cx).sunken))
                .tooltip(move |window, cx| Tooltip::new(tooltip_text.clone()).build(window, cx))
                .child(self.render_song_cover_styled(
                    song,
                    player_cover_size,
                    tokens::radius::COVER,
                    Vec::new(),
                ))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1()
                        .child(
                            div()
                                .text_size(px(tokens::BODY))
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
                                .child(match song.bilibili_subtitle() {
                                    Some(subtitle) => {
                                        div().truncate().child(subtitle).into_any_element()
                                    }
                                    None => self.song_artist_link(
                                        SharedString::from(format!(
                                            "player-bar-artist-{}",
                                            song.id
                                        )),
                                        song,
                                        cx,
                                    ),
                                }),
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
                                .text_size(px(tokens::BODY))
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
            .map(|(song, quality)| {
                if song.bilibili_part.is_some() {
                    crate::app::playback::display::bilibili_technical_info(
                        self.state.now_playing_audio_quality.as_ref(),
                    )
                } else {
                    playback_technical_info(song, quality)
                }
            });
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
                            .child(technical_info_chip("player-format", info.format, cx)),
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
                    .selected(queue_open)
                    .with_size(px(30.0))
                    .rounded_full()
                    .toggled(queue_open)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.state.settings.close();
                        this.state.queue_drawer_open = !this.state.queue_drawer_open;
                        if !this.state.queue_drawer_open {
                            this.clear_song_multi();
                        }
                        cx.notify();
                    })),
            )
            .child(self.render_volume_control(accent, cx))
            .into_any_element()
    }
}

fn technical_info_chip(
    id: &'static str,
    label: String,
    cx: &Context<NavidromeApp>,
) -> gpui_kit::AnyElement {
    let p = palette(cx);
    div()
        .id(id)
        .h(px(20.0))
        .max_w(px(64.0))
        .px_1p5()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(tokens::radius::CONTROL))
        .bg(p.sunken)
        .text_size(px(tokens::CAPTION))
        .text_color(p.text_secondary)
        .truncate()
        .child(label)
        .into_any_element()
}
