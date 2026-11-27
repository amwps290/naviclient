//! 当前播放页渲染。

use crate::app::prelude::*;

impl NavidromeApp {
    /// 渲染当前播放页，包括黑胶封面、歌词和歌曲操作。
    pub(in crate::app) fn render_now_playing(
        &self,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let Some(song) = &self.state.now_playing else {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .child(
                    div()
                        .text_xl()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(t!("No track playing")),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(palette(cx).text_secondary)
                        .child(t!("Choose a song from your library.")),
                )
                .into_any_element();
        };

        let viewport_width = f32::from(window.viewport_size().width);
        let viewport_height = f32::from(window.viewport_size().height);
        let max_cover_size = if viewport_width < 1_100.0 {
            300.0
        } else {
            380.0
        };
        let cover_size = (viewport_height - 300.0).clamp(220.0, max_cover_size);
        let playback = self.playback_for_display();
        let p = palette(cx);
        let serif = crate::typography::serif_font_family(cx);
        let radio_seed = song.clone();
        let radio_loading = self.song_radio_loading(&song.id);
        let display_line = self
            .state
            .lyrics
            .lyrics()
            .and_then(|lyrics| lyrics.display_line_index(playback.position));
        let display_line = self.lyrics_ui.timing.display_line(display_line);
        let lyrics = self.state.lyrics.lyrics();
        let lyrics_body = if self.state.lyrics.loading() && lyrics.is_none() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(p.text_secondary)
                .child(self.state.lyrics.status.message())
                .into_any_element()
        } else if let Some(error) = self.state.lyrics.error() {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(p.text_secondary)
                .child(self.state.lyrics.status.message())
                .child(div().text_xs().child(error.to_string()))
                .into_any_element()
        } else if let Some(lyrics) = lyrics.filter(|lyrics| !lyrics.lines.is_empty()) {
            let synced = lyrics.is_synced();
            let show_translation = self.config.show_lyrics_translation;
            let aligning = self.lyrics_ui.timing.is_aligning();
            v_flex()
                .id("lyrics-scroll")
                .flex_1()
                .min_h_0()
                .px_6()
                .overflow_y_scroll()
                .track_scroll(&self.lyrics_scroll_handle)
                .child(div().h(px(240.0)).flex_none())
                .children(lyrics.lines.iter().enumerate().map(|(index, lyric_line)| {
                    let current = synced && display_line == Some(index);
                    let distance = display_line
                        .map(|active| active.abs_diff(index))
                        .unwrap_or(usize::MAX);
                    let word_timing = lyric_line
                        .word_timing
                        .as_ref()
                        .filter(|_| self.config.lyrics.word_timing);
                    let text = word_timing
                        .map_or(&lyric_line.text, |words| &words.text)
                        .clone();
                    let primary = if current {
                        word_timing
                            .map(|words| {
                                super::word_lyrics::WordLyrics::new(
                                    words.clone(),
                                    u64::try_from(playback.position.as_millis())
                                        .unwrap_or(u64::MAX),
                                    p.text.opacity(0.55),
                                    p.brand,
                                )
                                .into_any_element()
                            })
                            .unwrap_or_else(|| div().child(text.clone()).into_any_element())
                    } else {
                        div().child(text).into_any_element()
                    };
                    let start_ms = lyric_line.start_ms;
                    let translation = lyric_line
                        .translation
                        .clone()
                        .filter(|text| show_translation && !text.is_empty());
                    let line = div()
                        .id(SharedString::from(format!(
                            "lyric-line-{}-{index}",
                            song.id
                        )))
                        .w_full()
                        .max_w(px(760.0))
                        .min_h(px(64.0))
                        .mx_auto()
                        .px_4()
                        .py_2()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_center()
                        .line_height(relative(1.45))
                        .when_some(start_ms, |this, start_ms| {
                            this.cursor_pointer()
                                // Mark the line that a click is about to align.
                                .when(aligning, |this| {
                                    this.rounded(px(tokens::radius::ROW))
                                        .hover(|style| style.bg(p.brand_soft))
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.click_lyric_line(index, start_ms, cx)
                                }))
                        })
                        // 原文与翻译作为一个整体参与高亮/点击；翻译行小号弱化色。
                        .child(v_flex().w_full().gap_1().child(primary).when_some(
                            translation,
                            |this, translation| {
                                this.child(
                                    div()
                                        .text_sm()
                                        .text_color(p.text_secondary)
                                        .child(translation),
                                )
                            },
                        ));

                    let line = line.font_family(serif.clone());
                    if current {
                        line.text_size(px(tokens::LYRIC_ACTIVE))
                            .font_weight(FontWeight::BOLD)
                            .text_color(p.brand)
                            .px_6()
                            .py_3()
                            .with_animation(
                                SharedString::from(format!("active-lyric-{}-{index}", song.id)),
                                Animation::new(Duration::from_millis(280))
                                    .with_easing(ease_out_quint()),
                                |this, delta| this.opacity(0.70 + delta * 0.30),
                            )
                            .into_any_element()
                    } else if synced {
                        line.text_size(px(tokens::LYRIC))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(p.text.opacity(lyric_opacity(distance)))
                            .into_any_element()
                    } else {
                        line.text_size(px(tokens::LYRIC))
                            .text_color(p.text.opacity(0.72))
                            .into_any_element()
                    }
                }))
                .child(div().h(px(280.0)).flex_none())
                .into_any_element()
        } else {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(p.text_secondary)
                .child(self.state.lyrics.status.message())
                .child(
                    ghost_button("empty-lyrics-search")
                        .label(t!("Search lyrics"))
                        .compact_action()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_lyrics_dialog(
                                super::lyrics_search::LyricsDialog::Search,
                                window,
                                cx,
                            )
                        })),
                )
                .into_any_element()
        };

        let layout = self.shell_layout(window);
        let player_clearance = layout.player_clearance();
        let halo_color = self
            .now_playing_cover_palette()
            .map(|(_, accent)| {
                hsla(
                    accent.h,
                    accent.s.min(0.50),
                    if cx.theme().is_dark() { 0.48 } else { 0.60 },
                    if cx.theme().is_dark() { 0.16 } else { 0.14 },
                )
            })
            .unwrap_or_else(|| p.brand.opacity(0.10));

        h_flex()
            .size_full()
            .min_h_0()
            .px_8()
            .pt_6()
            .pb(px(24.0 + player_clearance))
            .gap_8()
            .items_center()
            .child(
                v_flex()
                    .w(px((cover_size + 40.0).max(320.0)))
                    .flex_none()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .relative()
                            .size(px(cover_size))
                            .flex_none()
                            .rounded_full()
                            .shadow(vec![
                                box_shadow(
                                    px(0.0),
                                    px(0.0),
                                    px(cover_size * 0.22),
                                    px(cover_size * 0.015),
                                    halo_color,
                                ),
                                box_shadow(
                                    px(0.0),
                                    px(18.0),
                                    px(44.0),
                                    px(-8.0),
                                    hsla(
                                        0.0,
                                        0.0,
                                        0.0,
                                        if cx.theme().is_dark() { 0.48 } else { 0.18 },
                                    ),
                                ),
                            ])
                            .child(self.render_vinyl_record(cover_size, cx)),
                    )
                    .child(
                        v_flex()
                            .items_center()
                            .gap_1()
                            .child(serif_heading(
                                song.title.clone(),
                                tokens::NOW_PLAYING_TITLE,
                                cx,
                            ))
                            .child(
                                div()
                                    .text_size(px(tokens::BODY))
                                    .text_color(p.brand)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.song_artist_link(
                                        SharedString::from(format!(
                                            "now-playing-artist-{}",
                                            song.id
                                        )),
                                        song,
                                        cx,
                                    )),
                            )
                            .when(
                                !song.album.is_empty() || song.bilibili_subtitle().is_some(),
                                |this| {
                                    this.child(
                                        div()
                                            .text_size(px(tokens::META))
                                            .text_color(p.text_secondary)
                                            .child(self.song_album_link(
                                                SharedString::from(format!(
                                                    "now-playing-album-{}",
                                                    song.id
                                                )),
                                                song.album.clone(),
                                                song,
                                                cx,
                                            )),
                                    )
                                },
                            )
                            .when(song.bilibili_part.is_none(), |this| {
                                let disabled = self
                                    .source_for_song(song)
                                    .is_none_or(|api| !api.capabilities().recommendations)
                                    || radio_loading;
                                this.child(
                                    capsule_button("now-playing-song-radio")
                                        .icon(PlayerIcon::Play)
                                        .label(if radio_loading {
                                            t!("Building song radio…")
                                        } else {
                                            t!("Start song radio")
                                        })
                                        .disabled_tonal(disabled)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.start_song_radio(radio_seed.clone());
                                            cx.notify();
                                        })),
                                )
                            }),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .pl_6()
                    .child(self.render_lyrics_header(cx))
                    .child(lyrics_body),
            )
            .into_any_element()
    }
}

fn lyric_opacity(distance: usize) -> f32 {
    match distance {
        1 => 0.78,
        2 => 0.68,
        3 => 0.6,
        _ => 0.55,
    }
}

#[cfg(test)]
mod tests;
