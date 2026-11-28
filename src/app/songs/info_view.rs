//! Read-only track metadata dialog and deferred menu focus handoff.

use crate::app::playback::display::{
    bilibili_technical_info, format_file_size, playback_technical_info,
};
use crate::app::prelude::*;
use crate::app::sound::loudness_state::ReplayGainSource;

impl NavidromeApp {
    /// 打开“曲目信息”对话框。对话框挂在窗口全局层，菜单入口先让
    /// 菜单完成关闭再挂载，避免其焦点恢复吞掉 Enter/Escape。
    pub(in crate::app) fn open_song_info(
        &mut self,
        song: Song,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.song_info = Some(song);
        if !window.has_active_dialog(cx) {
            let app = cx.entity().downgrade();
            let render_app = app.clone();
            window.defer(cx, move |window, cx| {
                window.open_dialog(cx, move |dialog, _, cx| {
                    render_app
                        .update(cx, |this, cx| {
                            let dialog = this.render_song_info_dialog(dialog, cx);
                            crate::app::style::surface::raised_dialog(dialog, cx)
                        })
                        .expect("app owns song info dialog")
                });
            });
        }
        cx.notify();
    }

    /// 曲目信息对话框：服务器元数据全览（标签、音轨号、时长、格式、
    /// 码率、文件大小、路径）以及当前播放链路诊断，支持长字段换行与单项复制。
    fn render_song_info_dialog(
        &self,
        dialog: gpui_kit::component::dialog::Dialog,
        cx: &Context<Self>,
    ) -> gpui_kit::component::dialog::Dialog {
        let Some(song) = &self.state.song_info else {
            return dialog;
        };
        let p = palette(cx);
        let dark = cx.theme().is_dark();
        let mono = cx.theme().mono_font_family.clone();
        let panel_bg = if dark {
            hsla(0.0, 0.0, 1.0, 0.035)
        } else {
            p.sunken.opacity(0.65)
        };
        let panel_border = if dark {
            hsla(0.0, 0.0, 1.0, 0.08)
        } else {
            p.border.opacity(0.82)
        };
        let hairline = if dark {
            hsla(0.0, 0.0, 1.0, 0.06)
        } else {
            p.border.opacity(0.72)
        };
        let is_now_playing = self
            .state
            .now_playing
            .as_ref()
            .is_some_and(|current| current.id == song.id && current.source == song.source);

        let info_row = |label: &str,
                        value: String,
                        use_mono: bool,
                        copy_id: Option<&'static str>|
         -> gpui_kit::AnyElement {
            let copy_value = value.clone();
            h_flex()
                .w_full()
                .px_3()
                .py_2()
                .gap_3()
                .items_start()
                .child(
                    div()
                        .w(px(92.0))
                        .flex_none()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.text_muted)
                        .pt(px(2.0))
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_sm()
                        .when(use_mono, |this| {
                            this.text_xs().font_family(mono.clone()).pt(px(2.0))
                        })
                        .text_color(p.text)
                        .whitespace_normal()
                        .child(value),
                )
                .when_some(copy_id, |this, id| {
                    this.child(
                        ghost_button(id)
                            .label(t!("Copy"))
                            .dense_action()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.copy_text_to_clipboard(copy_value.clone(), cx);
                            })),
                    )
                })
                .into_any_element()
        };

        let wrap_rows = |rows: Vec<gpui_kit::AnyElement>| {
            let mut card = v_flex()
                .w_full()
                .rounded(px(tokens::radius::COVER))
                .bg(panel_bg)
                .border_1()
                .border_color(panel_border)
                .overflow_hidden();
            for (index, row) in rows.into_iter().enumerate() {
                if index > 0 {
                    card = card.child(div().w_full().h(px(1.0)).bg(hairline));
                }
                card = card.child(row);
            }
            card
        };

        // Compact artwork header card with key audio badges.
        let mut badges: Vec<(String, bool)> = Vec::new();
        if let Some(suffix) = song.suffix.as_deref().filter(|s| !s.is_empty()) {
            badges.push((suffix.to_uppercase(), true));
        }
        if let Some(bit_rate) = song.bit_rate {
            badges.push((format!("{bit_rate} kbps"), false));
        }
        if let Some(size) = song.size {
            badges.push((format_file_size(size.max(0) as u64), false));
        }
        badges.push((format_duration(song.duration), false));

        let subtitle = if let Some(part) = &song.bilibili_part {
            let mut bits = vec![part.uploader.clone()];
            if part.count > 1 {
                bits.push(format!("P{}/{}", part.index, part.count));
            }
            bits.join(" · ")
        } else if !song.artist.trim().is_empty() && !song.album.trim().is_empty() {
            format!("{} · {}", song.artist, song.album)
        } else if !song.artist.trim().is_empty() {
            song.artist.clone()
        } else {
            song.album.clone()
        };

        let header_card = h_flex()
            .w_full()
            .p_3()
            .gap_3p5()
            .items_center()
            .rounded(px(tokens::radius::COVER))
            .bg(panel_bg)
            .border_1()
            .border_color(panel_border)
            .child(self.render_song_cover_styled(
                song,
                64.0,
                tokens::radius::ROW,
                cover_shadow(false, cx),
            ))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .truncate()
                            .child(song.title.clone()),
                    )
                    .when(!subtitle.trim().is_empty(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(p.text_secondary)
                                .truncate()
                                .child(subtitle),
                        )
                    })
                    .child(h_flex().flex_wrap().items_center().gap_1().children(
                        badges.into_iter().map(|(label, accent)| {
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded_full()
                                .bg(if accent {
                                    p.brand_soft
                                } else if dark {
                                    hsla(0.0, 0.0, 1.0, 0.06)
                                } else {
                                    p.raised.opacity(0.85)
                                })
                                .text_xs()
                                .font_family(mono.clone())
                                .font_weight(if accent {
                                    FontWeight::SEMIBOLD
                                } else {
                                    FontWeight::NORMAL
                                })
                                .text_color(if accent { p.brand } else { p.text_secondary })
                                .child(label)
                        }),
                    )),
            );

        let mut rows = Vec::new();
        rows.push(info_row(&t!("Title"), song.title.clone(), false, None));
        if let Some(part) = &song.bilibili_part {
            rows.push(info_row(&t!("Uploader"), part.uploader.clone(), false, None));
            if !part.video_title.trim().is_empty() && part.video_title != song.title {
                rows.push(info_row(
                    &t!("Video title"),
                    part.video_title.clone(),
                    false,
                    None,
                ));
            }
            if !part.collection_title.trim().is_empty() && part.collection_title != song.title {
                rows.push(info_row(
                    &t!("Collection"),
                    part.collection_title.clone(),
                    false,
                    None,
                ));
            }
            if part.count > 1 {
                rows.push(info_row(
                    &t!("Part"),
                    format!("P{}/{}", part.index, part.count),
                    true,
                    None,
                ));
            }
        }
        if !song.artist.trim().is_empty() {
            rows.push(info_row(&t!("Artist"), song.artist.clone(), false, None));
        }
        if !song.album.trim().is_empty() {
            rows.push(info_row(&t!("Album"), song.album.clone(), false, None));
        }
        if let Some(genre) = song.genre.as_deref().filter(|g| !g.trim().is_empty()) {
            rows.push(info_row(&t!("Genre"), genre.to_string(), false, None));
        }
        if song.track.is_some() || song.disc.is_some() {
            let value = match (song.disc, song.track) {
                (Some(disc), Some(track)) => t!(
                    "Disc %{disc} · Track %{track}",
                    disc = disc,
                    track = format!("{track:02}")
                ),
                (Some(disc), None) => t!("Disc %{disc}", disc = disc),
                (None, Some(track)) => t!("Track %{track}", track = format!("{track:02}")),
                (None, None) => std::borrow::Cow::Borrowed(""),
            };
            rows.push(info_row(&t!("Position"), value.to_string(), false, None));
        }
        rows.push(info_row(
            &t!("Duration"),
            format_duration(song.duration),
            true,
            None,
        ));
        if song.suffix.as_deref().is_some_and(|s| !s.is_empty()) {
            let mut value = song.suffix.as_deref().unwrap_or_default().to_uppercase();
            if let Some(bit_rate) = song.bit_rate {
                value.push_str(&format!(" · {bit_rate} kbps"));
            }
            if let Some(size) = song.size {
                value.push_str(&format!(" · {}", format_file_size(size.max(0) as u64)));
            }
            rows.push(info_row(&t!("Format"), value, true, None));
        }
        if let Some(path) = &song.path {
            if !path.is_empty() {
                rows.push(info_row(
                    &t!("File"),
                    path.clone(),
                    true,
                    Some("song-info-copy-path"),
                ));
            }
        }
        rows.push(info_row(
            &t!("ID"),
            song.id.clone(),
            true,
            Some("song-info-copy-id"),
        ));
        let info_card = wrap_rows(rows);

        let playback_section = is_now_playing.then(|| {
            let playback = self.audio.state();
            let technical = if song.bilibili_part.is_some() {
                bilibili_technical_info(self.state.now_playing_audio_quality.as_ref())
            } else {
                playback_technical_info(
                    song,
                    self.state
                        .now_playing_quality
                        .unwrap_or(self.config.transcoding_quality),
                )
            };
            let quality_text = if technical.tooltip.trim().is_empty() {
                if technical.format.trim().is_empty() {
                    self.config.transcoding_quality.label().to_string()
                } else {
                    technical.format
                }
            } else {
                technical.tooltip
            };
            let (rg_source, factor) = self
                .state
                .loudness
                .describe(song, self.config.volume_normalization);
            let gain_db = if factor > 0.0 {
                20.0 * factor.log10()
            } else {
                0.0
            };
            let rg_text = match rg_source {
                ReplayGainSource::Off => t!("Off").to_string(),
                ReplayGainSource::TrackTag { gain_db, .. } => {
                    t!("Track tag (%{gain} dB)", gain = format!("{gain_db:+.1}")).to_string()
                }
                ReplayGainSource::AlbumTag { gain_db, .. } => {
                    t!("Album tag (%{gain} dB)", gain = format!("{gain_db:+.1}")).to_string()
                }
                ReplayGainSource::LocalMeasurement { .. } => {
                    t!("Measured (%{gain} dB)", gain = format!("{gain_db:+.1}")).to_string()
                }
                ReplayGainSource::Neutral => t!("Not analyzed (0.0 dB)").to_string(),
            };
            let output_name = playback
                .output_name
                .clone()
                .unwrap_or_else(|| t!("System default").to_string());
            let output_text = format!(
                "{output_name} · {} Hz · {} ch",
                playback.output_sample_rate, playback.output_channels
            );

            v_flex()
                .w_full()
                .gap_1p5()
                .child(
                    div()
                        .px_1()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.text_muted)
                        .child(t!("Playback chain")),
                )
                .child(wrap_rows(vec![
                    info_row(&t!("Streaming quality"), quality_text, false, None),
                    info_row(&t!("ReplayGain"), rg_text, true, None),
                    info_row(&t!("Audio output"), output_text, false, None),
                ]))
        });

        let copy_summary = if song.artist.trim().is_empty() {
            song.title.clone()
        } else {
            format!("{} - {}", song.title, song.artist)
        };
        dialog
            .title(t!("Track info"))
            .w(px(540.0))
            .close_button(true)
            .keyboard(true)
            .on_close(cx.listener(|this, _, _, cx| {
                this.state.song_info = None;
                cx.notify();
            }))
            .child(
                v_flex()
                    .gap_3()
                    .child(header_card)
                    .child(
                        v_flex()
                            .id("song-info-scroll")
                            .max_h(px(420.0))
                            .overflow_y_scroll()
                            .gap_3()
                            .child(info_card)
                            .children(playback_section),
                    )
                    .child(
                        h_flex()
                            .pt_1()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.text_muted)
                                    .child(if song.bilibili_part.is_some() {
                                        t!("Metadata from Bilibili.")
                                    } else {
                                        t!("Metadata from the Navidrome server.")
                                    }),
                            )
                            .child(
                                surface_button("song-info-copy-title-artist")
                                    .label(t!("Copy title & artist"))
                                    .compact_action()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.copy_text_to_clipboard(copy_summary.clone(), cx);
                                    })),
                            ),
                    ),
            )
    }
}
