//! 当前播放页渲染。

use super::*;

impl NavidromeApp {
    /// 渲染当前播放页，包括黑胶封面、歌词和歌曲操作。
    pub(super) fn render_now_playing(
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
                        .text_color(cx.theme().muted_foreground)
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
        let playback = self.audio.state();
        let accent = self.now_playing_accent(cx);
        let radio_seed = song.clone();
        let radio_loading = self.song_radio_loading(&song.id);
        let active_line = self
            .state
            .lyrics
            .as_ref()
            .and_then(|lyrics| lyrics.active_line_index(playback.position));
        let lyrics = self.state.lyrics.as_ref();
        let lyrics_body = if self.state.lyrics_loading {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(cx.theme().muted_foreground)
                .child(t!("Loading lyrics..."))
                .into_any_element()
        } else if let Some(error) = &self.state.lyrics_error {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(cx.theme().muted_foreground)
                .child(t!("Lyrics are unavailable for this track."))
                .child(div().text_xs().child(error.clone()))
                .into_any_element()
        } else if let Some(lyrics) = lyrics.filter(|lyrics| !lyrics.lines.is_empty()) {
            let synced = lyrics.is_synced();
            let show_translation = self.config.show_lyrics_translation;
            v_flex()
                .id("lyrics-scroll")
                .flex_1()
                .min_h_0()
                .px_6()
                .overflow_y_scroll()
                .track_scroll(&self.lyrics_scroll_handle)
                .child(div().h(px(240.0)).flex_none())
                .children(lyrics.lines.iter().enumerate().map(|(index, lyric_line)| {
                    let current = synced && active_line == Some(index);
                    let distance = active_line
                        .map(|active| active.abs_diff(index))
                        .unwrap_or(usize::MAX);
                    let text = lyric_line.text.clone();
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
                        .line_height(rems(1.45))
                        .when_some(start_ms, |this, start_ms| {
                            this.cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.seek_playback(Duration::from_millis(start_ms));
                                    cx.notify();
                                }))
                        })
                        // 原文与翻译作为一个整体参与高亮/点击；翻译行小号弱化色。
                        .child(v_flex().gap_1().child(div().child(text)).when_some(
                            translation,
                            |this, translation| {
                                this.child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(translation),
                                )
                            },
                        ));

                    if current {
                        line.text_2xl()
                            .font_weight(FontWeight::BOLD)
                            .text_color(accent)
                            .with_animation(
                                SharedString::from(format!("active-lyric-{}-{index}", song.id)),
                                Animation::new(Duration::from_millis(220))
                                    .with_easing(ease_out_quint()),
                                |this, delta| this.opacity(0.64 + delta * 0.36),
                            )
                            .into_any_element()
                    } else if synced {
                        let opacity = match distance {
                            1 => 0.68,
                            2 => 0.52,
                            3 => 0.4,
                            _ => 0.28,
                        };
                        line.text_lg()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(cx.theme().foreground.opacity(opacity))
                            .into_any_element()
                    } else {
                        line.text_lg()
                            .text_color(cx.theme().foreground.opacity(0.72))
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
                .text_color(cx.theme().muted_foreground)
                .child(t!("No lyrics are available for this track."))
                .into_any_element()
        };

        let rotation_phase = self.vinyl_cover.phase();
        h_flex()
            .size_full()
            .min_h_0()
            .px_8()
            .py_5()
            .gap_8()
            .items_center()
            .child(
                v_flex()
                    .w(px((cover_size + 40.0).max(320.0)))
                    .flex_none()
                    .items_center()
                    .gap_4()
                    .child(self.render_vinyl_record(
                        song.cover_art.as_deref(),
                        cover_size,
                        rotation_phase,
                        playback.active && !playback.paused,
                        cx,
                    ))
                    .child(
                        v_flex()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .text_xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(song.title.clone()),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(self.song_artist_link(
                                        SharedString::from(format!(
                                            "now-playing-artist-{}",
                                            song.id
                                        )),
                                        song,
                                        cx,
                                    )),
                            )
                            .when(!song.album.is_empty(), |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().muted_foreground)
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
                            })
                            .child(
                                Button::new("now-playing-song-radio")
                                    .icon(PlayerIcon::Play)
                                    .label(if radio_loading {
                                        t!("Building song radio…")
                                    } else {
                                        t!("Start song radio")
                                    })
                                    .outline()
                                    .disabled(self.api.is_none() || radio_loading)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.start_song_radio(radio_seed.clone());
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .pl_6()
                    .child(
                        v_flex()
                            .items_center()
                            .gap_1()
                            .pb_1()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(t!("Lyrics")),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground.opacity(0.82))
                                    .child(t!(
                                        "%{title} - %{artist}",
                                        title = song.title,
                                        artist = song.artist
                                    )),
                            ),
                    )
                    .child(lyrics_body),
            )
            .into_any_element()
    }
}
