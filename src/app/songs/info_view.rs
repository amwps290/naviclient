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
        let is_now_playing = self
            .state
            .now_playing
            .as_ref()
            .is_some_and(|current| current.id == song.id && current.source == song.source);

        let info_row =
            |content: gpui_kit::Div, label: &str, value: String, copy_id: Option<&'static str>| {
                let copy_value = value.clone();
                content.child(
                    h_flex()
                        .gap_3()
                        .items_start()
                        .child(
                            div()
                                .w(px(96.0))
                                .flex_none()
                                .text_sm()
                                .text_color(cx.theme().muted_foreground)
                                .child(label.to_string()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_sm()
                                .whitespace_normal()
                                .child(value),
                        )
                        .when_some(copy_id, |this, id| {
                            this.child(
                                ghost_button(id)
                                    .label(t!("Copy"))
                                    .compact_action()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.copy_text_to_clipboard(copy_value.clone(), cx);
                                    })),
                            )
                        }),
                )
            };

        let mut info = v_flex().gap_2();
        info = info_row(info, &t!("Title"), song.title.clone(), None);
        if let Some(part) = &song.bilibili_part {
            info = info_row(info, &t!("Uploader"), part.uploader.clone(), None);
            if !part.video_title.trim().is_empty() && part.video_title != song.title {
                info = info_row(info, &t!("Video title"), part.video_title.clone(), None);
            }
            if !part.collection_title.trim().is_empty() && part.collection_title != song.title {
                info = info_row(info, &t!("Collection"), part.collection_title.clone(), None);
            }
            if part.count > 1 {
                info = info_row(
                    info,
                    &t!("Part"),
                    format!("P{}/{}", part.index, part.count),
                    None,
                );
            }
        }
        if !song.artist.trim().is_empty() {
            info = info_row(info, &t!("Artist"), song.artist.clone(), None);
        }
        if !song.album.trim().is_empty() {
            info = info_row(info, &t!("Album"), song.album.clone(), None);
        }
        if let Some(genre) = song.genre.as_deref().filter(|g| !g.trim().is_empty()) {
            info = info_row(info, &t!("Genre"), genre.to_string(), None);
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
            info = info_row(info, &t!("Position"), value.to_string(), None);
        }
        info = info_row(info, &t!("Duration"), format_duration(song.duration), None);
        if song.suffix.as_deref().is_some_and(|s| !s.is_empty()) {
            let mut value = song.suffix.as_deref().unwrap_or_default().to_uppercase();
            if let Some(bit_rate) = song.bit_rate {
                value.push_str(&format!(" · {bit_rate} kbps"));
            }
            if let Some(size) = song.size {
                value.push_str(&format!(" · {}", format_file_size(size.max(0) as u64)));
            }
            info = info_row(info, &t!("Format"), value, None);
        }
        if let Some(path) = &song.path {
            if !path.is_empty() {
                info = info_row(info, &t!("File"), path.clone(), Some("song-info-copy-path"));
            }
        }
        info = info_row(info, &t!("ID"), song.id.clone(), Some("song-info-copy-id"));

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

            let mut chain = v_flex()
                .gap_2()
                .pt_2()
                .mt_1()
                .border_t_1()
                .border_color(p.border.opacity(0.6))
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(p.text_secondary)
                        .child(t!("Playback chain")),
                );
            chain = info_row(chain, &t!("Streaming quality"), quality_text, None);
            chain = info_row(chain, &t!("ReplayGain"), rg_text, None);
            chain = info_row(chain, &t!("Audio output"), output_text, None);
            chain
        });

        let copy_summary = if song.artist.trim().is_empty() {
            song.title.clone()
        } else {
            format!("{} - {}", song.title, song.artist)
        };
        dialog
            .title(t!("Track info"))
            .w(px(500.0))
            .close_button(true)
            .keyboard(true)
            .on_close(cx.listener(|this, _, _, cx| {
                this.state.song_info = None;
                cx.notify();
            }))
            .child(
                v_flex()
                    .gap_2()
                    .p_1()
                    .child(info)
                    .children(playback_section)
                    .child(
                        h_flex()
                            .pt_2()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
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
