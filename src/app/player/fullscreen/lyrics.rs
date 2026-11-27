//! Real timed and plain lyrics with luminous active focus, accent anchor, and depth fade.

use super::layout::FullscreenLayout;
use super::view::{salon_button, salon_palette};
use crate::app::prelude::*;

impl NavidromeApp {
    pub(super) fn render_fullscreen_lyrics(
        &self,
        layout: &FullscreenLayout,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = salon_palette();
        let chrome_opacity = self.fullscreen.state.opacity();
        let playback = self.playback_for_display();
        let lyrics = self.state.lyrics.lyrics();
        let synced = lyrics.is_some_and(|lyrics| lyrics.is_synced());
        let active = lyrics.and_then(|lyrics| lyrics.display_line_index(playback.position));
        let active = self.lyrics_ui.timing.display_line(active);
        let count = lyrics.map_or(0, |lyrics| lyrics.lines.len());
        let song_id = self
            .state
            .now_playing
            .as_ref()
            .map_or("empty", |song| song.id.as_str());
        let translated = self.fullscreen.translated;
        let serif = crate::typography::serif_font_family(cx);
        let body = if let Some(lyrics) = lyrics.filter(|lyrics| !lyrics.lines.is_empty()) {
            v_flex()
                .id("fullscreen-lyrics-scroll")
                .size_full()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&self.fullscreen.scroll)
                .on_scroll_wheel(cx.listener(move |this, _, _, cx| {
                    if synced {
                        this.state
                            .lyrics
                            .pause_follow_for_scroll(std::time::Instant::now());
                        cx.notify();
                    }
                }))
                // The spacers let the first and last cue reach the viewport center.
                .child(div().h(px(layout.body_height * 0.48)).flex_none())
                .children(lyrics.lines.iter().enumerate().map(|(index, line)| {
                    let current = synced && active == Some(index);
                    let distance = active.map_or(usize::MAX, |active| active.abs_diff(index));
                    let words = line
                        .word_timing
                        .as_ref()
                        .filter(|_| self.config.lyrics.word_timing);
                    let text = words.map_or(&line.text, |words| &words.text).clone();
                    let primary = if current {
                        words
                            .map(|words| {
                                crate::app::player::word_lyrics::WordLyrics::new(
                                    words.clone(),
                                    u64::try_from(playback.position.as_millis())
                                        .unwrap_or(u64::MAX),
                                    p.muted.opacity(0.75),
                                    p.active_lyric,
                                )
                                .into_any_element()
                            })
                            .unwrap_or_else(|| div().child(text.clone()).into_any_element())
                    } else {
                        div().child(text).into_any_element()
                    };
                    let translation = line
                        .translation
                        .clone()
                        .filter(|text| translated && !text.is_empty() && (current || !synced));

                    h_flex()
                        .id(SharedString::from(format!(
                            "fullscreen-lyric-{song_id}-{index}"
                        )))
                        .w_full()
                        .flex_none()
                        .items_start()
                        .gap_4()
                        .py(px(if layout.compact { 10.0 } else { 14.0 }))
                        .when_some(line.start_ms, |this, start| {
                            this.cursor_pointer()
                                .hover(|this| this.text_color(p.text))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.click_lyric_line(index, start, cx)
                                }))
                        })
                        .child(
                            div()
                                .w(px(3.0))
                                .h(px(if current {
                                    layout.active_lyric_size * 0.85
                                } else {
                                    0.0
                                }))
                                .mt(px(if current {
                                    layout.active_lyric_size * 0.24
                                } else {
                                    0.0
                                }))
                                .flex_none()
                                .rounded_full()
                                .when(current, |this| {
                                    this.bg(p.accent).shadow(vec![box_shadow(
                                        px(0.0),
                                        px(0.0),
                                        px(10.0),
                                        px(0.0),
                                        p.accent.opacity(0.45),
                                    )])
                                }),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap(px(8.0))
                                .font_family(serif.clone())
                                .line_height(relative(if current { 1.38 } else { 1.46 }))
                                .text_size(px(if current {
                                    layout.active_lyric_size
                                } else {
                                    layout.lyric_size
                                }))
                                .font_weight(if current {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::MEDIUM
                                })
                                .text_color(if current {
                                    p.active_lyric
                                } else {
                                    p.text.opacity(if !synced {
                                        0.68
                                    } else if distance <= 1 {
                                        0.52
                                    } else if distance == 2 {
                                        0.28
                                    } else {
                                        0.15
                                    })
                                })
                                .child(primary)
                                .when_some(translation, |this, translation| {
                                    this.child(
                                        div()
                                            .font_family(cx.theme().font_family.clone())
                                            .text_size(px(if layout.compact { 13.0 } else { 16.0 }))
                                            .line_height(relative(1.45))
                                            .font_weight(FontWeight::NORMAL)
                                            .text_color(p.secondary.opacity(0.85))
                                            .child(translation),
                                    )
                                }),
                        )
                        .into_any_element()
                }))
                .child(div().h(px(layout.body_height * 0.48)).flex_none())
                .into_any_element()
        } else {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .text_color(p.secondary)
                .text_size(px(15.0))
                .child(self.state.lyrics.status.message())
                .when_some(self.state.lyrics.error(), |this, error| {
                    this.child(
                        div()
                            .max_w(px(480.0))
                            .text_center()
                            .text_size(px(12.0))
                            .text_color(p.muted)
                            .child(error.to_string()),
                    )
                })
                .when(
                    self.state.now_playing.is_some() && !self.state.lyrics.loading(),
                    |this| {
                        this.child(div().h(px(34.0)).when(chrome_opacity > 0.0, |this| {
                            this.opacity(chrome_opacity).child(
                                salon_button(
                                    "fullscreen-refresh-lyrics",
                                    AppIcon::Refresh,
                                    t!("Refresh lyrics"),
                                    cx,
                                )
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.refresh_lyrics();
                                        cx.notify();
                                    },
                                )),
                            )
                        }))
                    },
                )
                .into_any_element()
        };

        v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .when(!layout.stacked, |this| this.h_full())
            .when(layout.stacked, |this| this.w_full())
            .child(
                div()
                    .h(px(26.0))
                    .w_full()
                    .flex_none()
                    .when(chrome_opacity > 0.0, |this| {
                        this.opacity(chrome_opacity).child(
                            h_flex()
                                .size_full()
                                .pl(px(19.0))
                                .items_center()
                                .justify_between()
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .items_center()
                                        .child(div().size(px(5.0)).rounded_full().bg(if synced {
                                            p.accent
                                        } else {
                                            p.muted
                                        }))
                                        .child(
                                            div()
                                                .text_size(px(12.0))
                                                .text_color(p.secondary)
                                                .child(t!("Lyrics")),
                                        )
                                        .when(count > 0, |this| {
                                            this.child(
                                                div()
                                                    .text_size(px(12.0))
                                                    .text_color(p.muted)
                                                    .child(format!(
                                                        "·  {:02} / {count}",
                                                        active.map_or(0, |index| index + 1)
                                                    )),
                                            )
                                        }),
                                )
                                .when(synced, |this| {
                                    this.child(
                                        div()
                                            .text_size(px(12.0))
                                            .text_color(p.muted.opacity(0.75))
                                            .child(t!("Click a lyric to seek")),
                                    )
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .mt_2()
                    .child(body),
            )
            .into_any_element()
    }
}
