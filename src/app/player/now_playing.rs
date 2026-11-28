//! 当前播放页渲染。

use crate::app::prelude::*;
use gpui_kit::component::menu::PopupMenuItem;

/// Viewport-derived geometry for the standard Now Playing vinyl stage.
/// Keeps the turntable anchored vertically even when titles span two lines.
#[derive(Clone, Copy, Debug)]
pub(super) struct NowPlayingStage {
    pub(super) compact: bool,
    pub(super) stage_max_width: f32,
    pub(super) left_width: f32,
    pub(super) cover_size: f32,
    pub(super) metadata_width: f32,
    pub(super) title_size: f32,
    pub(super) title_slot_height: f32,
    pub(super) column_gap: f32,
    pub(super) lyric_size: f32,
    pub(super) active_lyric_size: f32,
    pub(super) scroll_spacer: f32,
}

impl NowPlayingStage {
    pub(super) fn new(viewport_width: f32, viewport_height: f32, player_clearance: f32) -> Self {
        let width = viewport_width.max(1.0);
        let height = viewport_height.max(1.0);
        let compact = width < 1_080.0 || height < 740.0;
        let horizontal_padding = if compact { 28.0 } else { 48.0 };
        let top_padding = if compact { 16.0 } else { 24.0 };
        let bottom_padding = 20.0 + player_clearance;
        let available_height = (height - top_padding - bottom_padding).max(260.0);
        let stage_max_width = (width - horizontal_padding * 2.0)
            .max(320.0)
            .min(if width >= 1_800.0 { 1_440.0 } else { 1_260.0 });
        let column_gap = if compact {
            32.0
        } else {
            (stage_max_width * 0.055).clamp(40.0, 76.0)
        };
        let title_size: f32 = if compact {
            24.0
        } else {
            tokens::NOW_PLAYING_TITLE
        };
        // Fixed 2-line title budget (1.28 line-height + small breathing room) so long titles
        // never push the turntable upward.
        let title_slot_height = (title_size * 1.28 * 2.0).ceil();
        // Reserve space for the fixed metadata & action stack under the vinyl record:
        // gap (22) + album/tag (18) + 2-line title + artist/subtitle (40) + actions (36).
        let metadata_reserve = title_slot_height + if compact { 116.0 } else { 132.0 };
        let max_cover_by_height = (available_height - metadata_reserve).max(168.0);
        let max_cover_by_width = ((stage_max_width - column_gap) * 0.42).max(168.0);
        let cover_size = max_cover_by_height
            .min(max_cover_by_width)
            .clamp(168.0, if compact { 290.0 } else { 360.0 });
        let left_width = (cover_size + if compact { 32.0 } else { 44.0 })
            .max(280.0)
            .min((stage_max_width - column_gap) * 0.46);
        let metadata_width = cover_size.max(256.0).min(left_width);
        let active_lyric_size = if compact { 24.0 } else { tokens::LYRIC_ACTIVE };
        let lyric_size = if compact { 18.0 } else { tokens::LYRIC };
        let scroll_spacer = (available_height * 0.38).clamp(140.0, 260.0);
        Self {
            compact,
            stage_max_width,
            left_width,
            cover_size,
            metadata_width,
            title_size,
            title_slot_height,
            column_gap,
            lyric_size,
            active_lyric_size,
            scroll_spacer,
        }
    }
}

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

        let layout = self.shell_layout(window);
        let player_clearance = layout.player_clearance();
        let viewport_width = f32::from(window.viewport_size().width);
        let viewport_height = f32::from(window.viewport_size().height);
        let stage = NowPlayingStage::new(viewport_width, viewport_height, player_clearance);
        let cover_size = stage.cover_size;
        let playback = self.playback_for_display();
        let p = palette(cx);
        let dark = cx.theme().is_dark();
        let serif = crate::typography::serif_font_family(cx);
        let radio_seed = song.clone();
        let radio_loading = self.song_radio_loading(&song.id);
        let can_favorite = self.api.as_ref().is_some_and(|api| {
            api.capabilities().favorite_write
                && self
                    .source_for_song(song)
                    .is_some_and(|source| source.id() == api.id())
        });
        let accent_color = self
            .now_playing_cover_palette()
            .map(|(_, accent)| {
                hsla(
                    accent.h,
                    accent.s.clamp(0.28, 0.62),
                    if dark { 0.66 } else { 0.44 },
                    1.0,
                )
            })
            .unwrap_or(p.brand);
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
                .justify_start()
                .pl(px(22.0))
                .text_color(p.text_secondary)
                .child(self.state.lyrics.status.message())
                .into_any_element()
        } else if let Some(error) = self.state.lyrics.error() {
            v_flex()
                .flex_1()
                .items_start()
                .justify_center()
                .pl(px(22.0))
                .gap_2()
                .text_color(p.text_secondary)
                .child(self.state.lyrics.status.message())
                .child(div().text_xs().text_color(p.text_muted).child(error.to_string()))
                .into_any_element()
        } else if let Some(lyrics) = lyrics.filter(|lyrics| !lyrics.lines.is_empty()) {
            let synced = lyrics.is_synced();
            let show_translation = self.config.show_lyrics_translation;
            let aligning = self.lyrics_ui.timing.is_aligning();
            let manual_scroll_paused = synced
                && self
                    .state
                    .lyrics
                    .is_manual_scroll_paused(std::time::Instant::now());
            let app_for_line_menu = cx.entity().downgrade();
            let active_color = if dark { p.text } else { p.brand };
            let word_base_color = if dark {
                p.text.opacity(0.42)
            } else {
                p.text.opacity(0.45)
            };
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(
                    v_flex()
                        .id("lyrics-scroll")
                        .size_full()
                        .pr_4()
                        .overflow_y_scroll()
                        .track_scroll(&self.lyrics_scroll_handle)
                        .on_scroll_wheel(cx.listener(move |this, _, _, cx| {
                            if synced {
                                this.state
                                    .lyrics
                                    .pause_follow_for_scroll(std::time::Instant::now());
                                cx.notify();
                            }
                        }))
                        .child(div().h(px(stage.scroll_spacer)).flex_none())
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
                            let copy_main = text.clone();
                            let primary = if current {
                                word_timing
                                    .map(|words| {
                                        super::word_lyrics::WordLyrics::new(
                                            words.clone(),
                                            u64::try_from(playback.position.as_millis())
                                                .unwrap_or(u64::MAX),
                                            word_base_color,
                                            active_color,
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
                            let copy_translation = translation.clone();
                            let line_menu_app = app_for_line_menu.clone();
                            let hover_bg = if dark {
                                hsla(0.0, 0.0, 1.0, 0.045)
                            } else {
                                p.text.opacity(0.04)
                            };
                            let text_block = v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1()
                                .font_family(serif.clone())
                                .line_height(relative(if current { 1.38 } else { 1.48 }))
                                .text_size(px(if current {
                                    stage.active_lyric_size
                                } else {
                                    stage.lyric_size
                                }))
                                .font_weight(if current {
                                    FontWeight::BOLD
                                } else {
                                    FontWeight::MEDIUM
                                })
                                .text_color(if current {
                                    active_color
                                } else if synced {
                                    p.text.opacity(lyric_opacity(distance))
                                } else {
                                    p.text.opacity(0.76)
                                })
                                .child(primary)
                                .when_some(translation, |this, translation| {
                                    this.child(
                                        div()
                                            .font_family(cx.theme().font_family.clone())
                                            .text_size(px(if current { 14.0 } else { 13.0 }))
                                            .line_height(relative(1.45))
                                            .font_weight(FontWeight::NORMAL)
                                            .text_color(if current {
                                                p.text_secondary
                                            } else if synced {
                                                p.text_secondary
                                                    .opacity((lyric_opacity(distance) + 0.12).min(0.9))
                                            } else {
                                                p.text_secondary.opacity(0.82)
                                            })
                                            .child(translation),
                                    )
                                });

                            let anchor = div()
                                .w(px(3.0))
                                .h(px(if current {
                                    stage.active_lyric_size * 0.92
                                } else {
                                    0.0
                                }))
                                .mt(px(if current {
                                    stage.active_lyric_size * 0.22
                                } else {
                                    0.0
                                }))
                                .flex_none()
                                .rounded_full()
                                .when(current, |this| {
                                    this.bg(accent_color).shadow(vec![box_shadow(
                                        px(0.0),
                                        px(0.0),
                                        px(12.0),
                                        px(0.0),
                                        accent_color.opacity(if dark { 0.55 } else { 0.35 }),
                                    )])
                                });

                            let row = h_flex()
                                .id(SharedString::from(format!(
                                    "lyric-line-{}-{index}",
                                    song.id
                                )))
                                .w_full()
                                .max_w(px(740.0))
                                .flex_none()
                                .items_start()
                                .gap_3p5()
                                .px_3()
                                .py(px(if current { 12.0 } else { 9.0 }))
                                .rounded(px(tokens::radius::ROW))
                                .when_some(start_ms, |this, start_ms| {
                                    this.cursor_pointer()
                                        .hover(move |style| {
                                            if aligning {
                                                style.bg(p.brand_soft)
                                            } else {
                                                style.bg(hover_bg)
                                            }
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.click_lyric_line(index, start_ms, cx)
                                        }))
                                })
                                .context_menu(move |menu, _, cx| {
                                    let main = copy_main.clone();
                                    let trans = copy_translation.clone();
                                    line_menu_app
                                        .update(cx, |_, cx| {
                                            let main_for_copy = main.clone();
                                            let mut menu = menu.item(
                                                PopupMenuItem::new(t!("Copy line"))
                                                    .disabled(main.trim().is_empty())
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.copy_text_to_clipboard(
                                                                main_for_copy.clone(),
                                                                cx,
                                                            );
                                                        },
                                                    )),
                                            );
                                            if let Some(trans) = trans {
                                                let combined = format!("{main}\n{trans}");
                                                menu = menu.item(
                                                    PopupMenuItem::new(t!(
                                                        "Copy line with translation"
                                                    ))
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.copy_text_to_clipboard(
                                                            combined.clone(),
                                                            cx,
                                                        );
                                                    })),
                                                );
                                            }
                                            menu
                                        })
                                        .expect("app owns lyric line menu")
                                })
                                .child(anchor)
                                .child(text_block);

                            if current {
                                row.with_animation(
                                    SharedString::from(format!(
                                        "active-lyric-{}-{index}",
                                        song.id
                                    )),
                                    Animation::new(Duration::from_millis(260))
                                        .with_easing(ease_out_quint()),
                                    |this, delta| this.opacity(0.72 + delta * 0.28),
                                )
                                .into_any_element()
                            } else {
                                row.into_any_element()
                            }
                        }))
                        .child(div().h(px(stage.scroll_spacer + 40.0)).flex_none()),
                )
                .when(manual_scroll_paused, |this| {
                    this.child(
                        div().absolute().right_4().bottom_4().child(
                            capsule_button_sm("lyrics-return-current")
                                .label(t!("Back to current line"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.state.lyrics.resume_follow_now();
                                    cx.notify();
                                })),
                        ),
                    )
                })
                .into_any_element()
        } else {
            v_flex()
                .flex_1()
                .items_start()
                .justify_center()
                .pl(px(22.0))
                .gap_3()
                .text_color(p.text_secondary)
                .child(self.state.lyrics.status.message())
                .child(
                    surface_button("empty-lyrics-search")
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

        let halo_color = self
            .now_playing_cover_palette()
            .map(|(_, accent)| {
                hsla(
                    accent.h,
                    accent.s.min(0.55),
                    if dark { 0.46 } else { 0.62 },
                    if dark { 0.20 } else { 0.16 },
                )
            })
            .unwrap_or_else(|| p.brand.opacity(if dark { 0.14 } else { 0.10 }));
        let ambient_tint = self
            .now_playing_cover_palette()
            .map(|(base, _)| {
                hsla(
                    base.h,
                    base.s.min(0.45),
                    if dark { 0.34 } else { 0.72 },
                    if dark { 0.16 } else { 0.14 },
                )
            })
            .unwrap_or_else(|| p.brand.opacity(0.06));
        let vinyl_metal_tint = self
            .now_playing_cover_palette()
            .map(|(_, accent)| accent)
            .or(Some(p.brand));
        let has_album_meta = !song.album.is_empty() || song.bilibili_subtitle().is_some();
        let full_title = song.title.clone();

        div()
            .relative()
            .size_full()
            .min_h_0()
            .overflow_hidden()
            // Soft cover-reactive atmospheric wash anchored on the vinyl side.
            .child(div().absolute().inset_0().bg(linear_gradient(
                118.0,
                linear_color_stop(ambient_tint, 0.0),
                linear_color_stop(p.page.opacity(0.0), 0.52),
            )))
            .child(
                h_flex()
                    .relative()
                    .size_full()
                    .max_w(px(stage.stage_max_width))
                    .mx_auto()
                    .min_h_0()
                    .px(px(if stage.compact { 28.0 } else { 48.0 }))
                    .pt(px(if stage.compact { 16.0 } else { 24.0 }))
                    .pb(px(20.0 + player_clearance))
                    .gap(px(stage.column_gap))
                    .items_center()
                    .child(
                        v_flex()
                            .w(px(stage.left_width))
                            .flex_none()
                            .items_center()
                            .child(
                                v_flex()
                                    .w(px(stage.metadata_width))
                                    .min_w_0()
                                    .items_center()
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
                                                    px(cover_size * 0.26),
                                                    px(cover_size * 0.02),
                                                    halo_color,
                                                ),
                                                box_shadow(
                                                    px(0.0),
                                                    px(20.0),
                                                    px(48.0),
                                                    px(-8.0),
                                                    hsla(
                                                        0.0,
                                                        0.0,
                                                        0.0,
                                                        if dark { 0.54 } else { 0.20 },
                                                    ),
                                                ),
                                            ])
                                            .child(self.render_vinyl_record_styled(
                                                cover_size,
                                                0.46,
                                                vinyl_metal_tint,
                                                cx,
                                            )),
                                    )
                                    .child(
                                        v_flex()
                                            .w_full()
                                            .min_w_0()
                                            .mt(px(if stage.compact { 18.0 } else { 24.0 }))
                                            .gap_1p5()
                                            .child(
                                                h_flex()
                                                    .w_full()
                                                    .h(px(18.0))
                                                    .flex_none()
                                                    .items_center()
                                                    .gap_2()
                                                    .when(has_album_meta, |this| {
                                                        this.child(
                                                            div()
                                                                .w(px(16.0))
                                                                .h(px(1.5))
                                                                .flex_none()
                                                                .rounded_full()
                                                                .bg(accent_color.opacity(0.72)),
                                                        )
                                                        .child(
                                                            div()
                                                                .flex_1()
                                                                .min_w_0()
                                                                .text_size(px(tokens::CAPTION))
                                                                .text_color(p.text_muted)
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
                                                    }),
                                            )
                                            .child(
                                                // Fixed 2-line title container: clamps long titles
                                                // cleanly with ellipsis and tooltip without shifting
                                                // the vinyl record above.
                                                div()
                                                    .id("now-playing-song-title")
                                                    .w_full()
                                                    .h(px(stage.title_slot_height))
                                                    .flex_none()
                                                    .overflow_hidden()
                                                    .font_family(serif.clone())
                                                    .font_weight(FontWeight::BOLD)
                                                    .text_size(px(stage.title_size))
                                                    .line_height(relative(1.28))
                                                    .text_color(p.text)
                                                    .text_ellipsis()
                                                    .line_clamp(2)
                                                    .tooltip(move |window, cx| {
                                                        Tooltip::new(full_title.clone())
                                                            .build(window, cx)
                                                    })
                                                    .child(song.title.clone()),
                                            )
                                            .child(
                                                h_flex()
                                                    .w_full()
                                                    .min_w_0()
                                                    .h(px(22.0))
                                                    .flex_none()
                                                    .items_center()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .min_w_0()
                                                            .text_size(px(tokens::BODY))
                                                            .text_color(accent_color)
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .child(self.song_artist_link(
                                                                SharedString::from(format!(
                                                                    "now-playing-artist-{}",
                                                                    song.id
                                                                )),
                                                                song,
                                                                cx,
                                                            )),
                                                    ),
                                            )
                                            .child(
                                                h_flex()
                                                    .w_full()
                                                    .h(px(36.0))
                                                    .mt_2()
                                                    .flex_none()
                                                    .items_center()
                                                    .gap_2()
                                                    .when(can_favorite, |this| {
                                                        this.child(self.favorite_button_sized(
                                                            FavoriteKind::Song,
                                                            &song.id,
                                                            tokens::ICON_BUTTON_MD,
                                                            cx,
                                                        ))
                                                    })
                                                    .when(song.bilibili_part.is_none(), |this| {
                                                        let disabled = self
                                                            .source_for_song(song)
                                                            .is_none_or(|api| {
                                                                !api.capabilities().recommendations
                                                            })
                                                            || radio_loading;
                                                        this.child(
                                                            capsule_button_sm(
                                                                "now-playing-song-radio",
                                                            )
                                                            .icon(PlayerIcon::Play)
                                                            .label(if radio_loading {
                                                                t!("Building song radio…")
                                                            } else {
                                                                t!("Start song radio")
                                                            })
                                                            .disabled_tonal(disabled)
                                                            .on_click(cx.listener(
                                                                move |this, _, _, cx| {
                                                                    this.start_song_radio(
                                                                        radio_seed.clone(),
                                                                    );
                                                                    cx.notify();
                                                                },
                                                            )),
                                                        )
                                                    }),
                                            ),
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .min_h_0()
                            .child(self.render_lyrics_header(cx))
                            .child(lyrics_body),
                    ),
            )
            .into_any_element()
    }
}

fn lyric_opacity(distance: usize) -> f32 {
    match distance {
        1 => 0.62,
        2 => 0.42,
        3 => 0.30,
        _ => 0.22,
    }
}

#[cfg(test)]
mod tests;
