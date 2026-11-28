//! Lyrics actions, candidate preview and per-song timing controls.

use super::lyrics_search::LyricsDialog;
use crate::app::prelude::*;
use crate::lyrics::{
    matching,
    provider::{ProviderId, Record},
    service::LyricsStatus,
    LyricsOrigin,
};
use gpui_kit::component::{dialog::Dialog, menu::PopupMenuItem};

impl LyricsStatus {
    pub(in crate::app) fn message(self) -> SharedString {
        match self {
            Self::ReadingSource => t!("Reading music source lyrics…"),
            Self::SearchingOnline => t!("Searching online lyrics…"),
            Self::NoMatch => t!("No matching lyrics found."),
            Self::MetadataNeeded => t!("Enter a song title and artist to search online lyrics."),
            Self::TitleNeeded => t!("Enter a song title to search lyrics."),
            Self::SourcesNeeded => t!("Choose a lyrics source in Settings."),
            Self::Ambiguous => t!("Several versions match. Choose lyrics for this audio."),
            Self::SourceUnavailable => t!("Could not load lyrics from the music source."),
            Self::SourceTimedOut => t!("The music source lyrics request timed out."),
            Self::Unavailable => t!("The lyrics service is temporarily unavailable."),
            Self::RateLimited(seconds) => t!(
                "Lyrics service is busy. Retry in %{seconds} seconds.",
                seconds = seconds
            ),
            Self::Instrumental => t!("Instrumental track, no lyrics."),
            _ => t!("No lyrics are available for this track."),
        }
        .into()
    }
}

impl ProviderId {
    pub(in crate::app) fn label(self) -> SharedString {
        match self {
            Self::Lrclib => t!("LRCLIB"),
            Self::Kugou => t!("Kugou"),
            Self::Netease => t!("NetEase Cloud Music"),
        }
        .into()
    }
}

fn record_duration(record: &Record) -> SharedString {
    if record.duration.is_finite() && record.duration > 0.0 {
        format_duration(Some(record.duration.round().min(i32::MAX as f64) as i32)).into()
    } else {
        t!("Unknown duration").into()
    }
}

impl NavidromeApp {
    pub(in crate::app) fn render_lyrics_header(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let dark = cx.theme().is_dark();
        let document = self.state.lyrics.document.as_ref();
        let lyrics = self.state.lyrics.lyrics();
        let synced = lyrics.is_some_and(|l| l.is_synced());
        let original_synced = document.is_some_and(|d| d.lyrics.is_synced());
        let plain = document.is_some_and(|d| d.plain);
        let personal = document.is_some_and(|d| d.manual || d.offset_ms != 0 || d.plain);
        let word_timing = self.config.lyrics.word_timing;
        let show_translation = self.config.show_lyrics_translation;
        let has_translation = lyrics.is_some_and(|l| {
            l.lines.iter().any(|line| {
                line.translation
                    .as_deref()
                    .is_some_and(|text| !text.trim().is_empty())
            })
        });
        let has_lyrics_text =
            lyrics.is_some_and(|l| l.lines.iter().any(|line| !line.text.trim().is_empty()));
        let line_count = lyrics.map_or(0, |l| l.lines.len());
        let active_line = lyrics
            .and_then(|l| l.display_line_index(self.playback_for_display().position))
            .and_then(|index| self.lyrics_ui.timing.display_line(Some(index)));
        let timing_open = self.lyrics_ui.timing.is_open();
        // A closed bar still tells the listener that this song is shifted.
        let shift = document
            .filter(|d| synced && !timing_open && d.offset_ms != 0)
            .map(|d| super::lyrics_timing::shift_label(d.offset_ms));
        let app = cx.entity().downgrade();
        let source = document.map(|d| {
            let origin = match d.origin {
                LyricsOrigin::Native => t!("Music source"),
                LyricsOrigin::Lrclib => t!("LRCLIB"),
                LyricsOrigin::Kugou => t!("Kugou"),
                LyricsOrigin::Netease => t!("NetEase Cloud Music"),
            };
            let format = if d.instrumental {
                t!("Instrumental")
            } else if !plain && word_timing && d.lyrics.has_word_timing() {
                t!("Word-by-word lyrics")
            } else if synced {
                t!("Synced lyrics")
            } else {
                t!("Plain text lyrics")
            };
            let mut label = format!("{origin} · {format}");
            if d.manual {
                let selection = if d.temporary {
                    t!("Selected for this session")
                } else {
                    t!("Remembered selection")
                };
                label.push_str(&format!(" · {selection}"));
            }
            label
        });
        let pill_bg = if dark {
            hsla(0.0, 0.0, 1.0, 0.04)
        } else {
            p.raised.opacity(0.65)
        };
        let pill_border = if dark {
            hsla(0.0, 0.0, 1.0, 0.08)
        } else {
            p.border.opacity(0.75)
        };
        v_flex()
            .w_full()
            .max_w(px(740.0))
            .gap_1p5()
            .pb_2()
            .pl_3()
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .child(
                        h_flex()
                            .min_w_0()
                            .flex_1()
                            .items_center()
                            .gap_2()
                            .child(div().size(px(6.0)).flex_none().rounded_full().bg(if synced {
                                p.brand
                            } else {
                                p.text_muted.opacity(0.6)
                            }))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(p.text_secondary)
                                    .child(source.unwrap_or_else(|| t!("Lyrics").to_string())),
                            )
                            .when(line_count > 0 && synced, |this| {
                                this.child(
                                    div()
                                        .flex_none()
                                        .text_xs()
                                        .text_color(p.text_muted)
                                        .child(format!(
                                            "· {:02}/{line_count}",
                                            active_line.map_or(0, |index| index + 1)
                                        )),
                                )
                            })
                            .when_some(shift, |this, shift| {
                                this.child(
                                    div()
                                        .id("lyrics-shift")
                                        .flex_none()
                                        .cursor_pointer()
                                        .text_xs()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.brand)
                                        .child(format!("· {shift}"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.toggle_lyrics_timing(cx)
                                        })),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .px_1p5()
                            .py_0p5()
                            .items_center()
                            .gap_0p5()
                            .rounded_full()
                            .bg(pill_bg)
                            .border_1()
                            .border_color(pill_border)
                            .when(has_translation, |this| {
                                this.child(
                                    icon_button(
                                        "now-playing-translation",
                                        AppIcon::Translation,
                                        t!("Show translation"),
                                        tokens::ICON_BUTTON_XS,
                                    )
                                    .custom(soft_toggle_style(show_translation, cx))
                                    .selected(show_translation)
                                    .toggled(show_translation)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.set_show_lyrics_translation(!show_translation, cx)
                                    })),
                                )
                            })
                            .child(
                                ghost_button("change-lyrics")
                                    .label(t!("Change lyrics"))
                                    .compact_action()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_lyrics_dialog(LyricsDialog::Search, window, cx)
                                    })),
                            )
                            .child(
                                icon_button(
                                    "lyrics-actions",
                                    IconName::Ellipsis,
                                    t!("Lyrics actions"),
                                    tokens::ICON_BUTTON_XS,
                                )
                                .dropdown_menu(
                                    move |menu, _, cx| {
                                        app.update(cx, |_, cx| {
                                            let mut menu = menu
                                                .item(
                                                    PopupMenuItem::new(t!("Word-by-word lyrics"))
                                                        .checked(word_timing)
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.set_word_lyrics(
                                                                    !word_timing,
                                                                    cx,
                                                                )
                                                            },
                                                        )),
                                                )
                                                .item(
                                                    PopupMenuItem::new(t!("Show translation"))
                                                        .checked(show_translation)
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.set_show_lyrics_translation(
                                                                    !show_translation,
                                                                    cx,
                                                                )
                                                            },
                                                        )),
                                                )
                                                .item(
                                                    PopupMenuItem::new(t!(
                                                        "Search / change lyrics"
                                                    ))
                                                    .on_click(cx.listener(|this, _, window, cx| {
                                                        this.open_lyrics_dialog(
                                                            LyricsDialog::Search,
                                                            window,
                                                            cx,
                                                        )
                                                    })),
                                                )
                                                .item(
                                                    PopupMenuItem::new(t!("Lyrics timing"))
                                                        .checked(timing_open)
                                                        .disabled(!synced)
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.toggle_lyrics_timing(cx)
                                                        })),
                                                )
                                                .item(
                                                    PopupMenuItem::new(t!("Display as plain text"))
                                                        .checked(plain)
                                                        .disabled(!original_synced)
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.change_lyrics_display(
                                                                    |d| d.plain = !plain,
                                                                    cx,
                                                                )
                                                            },
                                                        )),
                                                )
                                                .item(
                                                    PopupMenuItem::new(t!("Copy lyrics"))
                                                        .disabled(!has_lyrics_text)
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.copy_all_lyrics(cx)
                                                        })),
                                                )
                                                .item(
                                                    PopupMenuItem::new(t!("Retry lyrics lookup"))
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.refresh_lyrics();
                                                            cx.notify();
                                                        })),
                                                );
                                            if personal {
                                                menu = menu.item(
                                                    PopupMenuItem::new(t!(
                                                        "Restore automatic selection"
                                                    ))
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.restore_lyrics_selection(cx)
                                                    })),
                                                );
                                            }
                                            menu
                                        })
                                        .expect("app owns lyrics menu")
                                    },
                                ),
                            ),
                    ),
            )
            .when_some(self.state.lyrics.warning, |this, status| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(p.text_secondary)
                        .child(status.message()),
                )
            })
            .children(self.render_lyrics_timing_bar(cx))
            .into_any_element()
    }

    pub(in crate::app) fn render_lyrics_dialog(
        &self,
        dialog: Dialog,
        window: &Window,
        cx: &Context<Self>,
    ) -> Dialog {
        let p = palette(cx);
        let dark = cx.theme().is_dark();
        let compact = f32::from(window.viewport_size().width) < 820.0;
        let viewport_height = f32::from(window.viewport_size().height);
        let ui = &self.lyrics_ui;
        let enabled = self.config.lyrics.enabled;
        let source = ui.source;
        let sources = self.config.lyrics.enabled_sources();
        let panel_bg = if dark {
            hsla(0.0, 0.0, 1.0, 0.03)
        } else {
            p.sunken.opacity(0.62)
        };
        let panel_border = if dark {
            hsla(0.0, 0.0, 1.0, 0.08)
        } else {
            p.border.opacity(0.85)
        };
        let hairline = if dark {
            hsla(0.0, 0.0, 1.0, 0.07)
        } else {
            p.border.opacity(0.75)
        };
        let badge_bg = if dark {
            hsla(0.0, 0.0, 1.0, 0.06)
        } else {
            p.raised.opacity(0.85)
        };
        let duration_label = ui
            .query
            .duration
            .map(|d| {
                t!(
                    "Current audio: %{duration}",
                    duration = format_duration(Some(d as i32))
                )
                .to_string()
            })
            .unwrap_or_else(|| t!("Unknown duration").to_string());

        let field = |label: SharedString, input: &Entity<InputState>| {
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(p.text_muted)
                        .child(label),
                )
                .child(Input::new(input))
        };

        let search_inputs = if compact {
            v_flex()
                .gap_2()
                .child(
                    h_flex()
                        .gap_2p5()
                        .child(field(t!("Song title").into(), &ui.title))
                        .child(field(t!("Artist (optional)").into(), &ui.artist)),
                )
                .child(
                    h_flex()
                        .gap_2p5()
                        .items_end()
                        .child(field(t!("Album (optional)").into(), &ui.album))
                        .child(
                            primary_button("search-online-lyrics")
                                .label(t!("Search lyrics"))
                                .compact_action()
                                .loading(ui.request.is_some())
                                .disabled(!enabled)
                                .on_click(cx.listener(|this, _, _, cx| this.search_lyrics(cx))),
                        ),
                )
                .into_any_element()
        } else {
            h_flex()
                .gap_2p5()
                .items_end()
                .child(field(t!("Song title").into(), &ui.title))
                .child(field(t!("Artist (optional)").into(), &ui.artist))
                .child(field(t!("Album (optional)").into(), &ui.album))
                .child(
                    primary_button("search-online-lyrics")
                        .label(t!("Search lyrics"))
                        .compact_action()
                        .loading(ui.request.is_some())
                        .disabled(!enabled)
                        .on_click(cx.listener(|this, _, _, cx| this.search_lyrics(cx))),
                )
                .into_any_element()
        };

        let source_pills = h_flex()
            .flex_wrap()
            .items_center()
            .gap_1p5()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.text_muted)
                    .pr_1()
                    .child(t!("Lyrics source")),
            )
            .children(
                std::iter::once(None)
                    .chain(sources.iter().copied().map(Some))
                    .map(|choice| {
                        let active = choice == source;
                        let label: SharedString = choice
                            .map(ProviderId::label)
                            .unwrap_or_else(|| t!("All enabled sources").into());
                        let id = match choice {
                            Some(provider) => {
                                SharedString::from(format!("lyrics-source-{}", provider.key()))
                            }
                            None => SharedString::from("lyrics-source-all"),
                        };
                        pill_filter_button(id, label, active, cx)
                            .h(px(26.0))
                            .px_2p5()
                            .text_xs()
                            .disabled(!enabled || sources.is_empty())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.lyrics_ui.source = choice;
                                this.search_lyrics(cx);
                            }))
                    }),
            );

        let fields = v_flex()
            .w_full()
            .p_3()
            .gap_2p5()
            .rounded(px(tokens::radius::COVER))
            .bg(panel_bg)
            .border_1()
            .border_color(panel_border)
            .child(search_inputs)
            .child(
                h_flex()
                    .w_full()
                    .flex_wrap()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .pt_2()
                    .border_t_1()
                    .border_color(hairline)
                    .child(source_pills)
                    .child(
                        div()
                            .flex_none()
                            .px_2()
                            .py_0p5()
                            .rounded_full()
                            .bg(badge_bg)
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(p.text_secondary)
                            .child(duration_label),
                    ),
            );

        let visible_records: Vec<(usize, &Record)> = ui
            .records
            .iter()
            .enumerate()
            .filter(|(_, record)| !ui.word_only || record.lyrics().has_word_timing())
            .collect();
        let visible_count = visible_records.len();

        let candidates = v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .rounded(px(tokens::radius::COVER))
            .bg(panel_bg)
            .border_1()
            .border_color(panel_border)
            .overflow_hidden()
            .child(
                h_flex()
                    .flex_none()
                    .px_3()
                    .py_2()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(hairline)
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1p5()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(p.text)
                                    .child(t!("Candidates")),
                            )
                            .when(visible_count > 0, |this| {
                                this.child(
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded_full()
                                        .bg(badge_bg)
                                        .text_xs()
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .text_color(p.text_secondary)
                                        .child(visible_count.to_string()),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1p5()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(p.text_secondary)
                                    .child(t!("Only word-by-word lyrics")),
                            )
                            .child(
                                Switch::new("word-lyrics-candidates-only")
                                    .checked(ui.word_only)
                                    .accessibility_label(t!("Only word-by-word lyrics"))
                                    .on_change(cx.listener(|this, checked, _, cx| {
                                        this.lyrics_ui.word_only = *checked;
                                        let keep_selected =
                                            this.lyrics_ui.selected.filter(|&index| {
                                                this.lyrics_ui.records.get(index).is_some_and(
                                                    |record| {
                                                        !*checked
                                                            || record.lyrics().has_word_timing()
                                                    },
                                                )
                                            });
                                        this.lyrics_ui.selected = keep_selected.or_else(|| {
                                            this.lyrics_ui.records.iter().position(|record| {
                                                !*checked || record.lyrics().has_word_timing()
                                            })
                                        });
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .id("lyrics-candidates")
                    .flex_1()
                    .min_h_0()
                    .p_2()
                    .overflow_y_scroll()
                    .track_scroll(&ui.candidates_scroll)
                    .gap_1p5()
                    .children(visible_records.into_iter().map(|(index, record)| {
                        let info = matching::assess(&ui.query, record);
                        let is_selected = ui.selected == Some(index);
                        let format_label = if record.instrumental {
                            t!("Instrumental")
                        } else if record.lyrics().has_word_timing() {
                            t!("Word-by-word lyrics")
                        } else if record.lyrics().is_synced() {
                            t!("Synced lyrics")
                        } else {
                            t!("Plain text lyrics")
                        };
                        let is_word_timed = !record.instrumental && record.lyrics().has_word_timing();
                        let tag = |text: SharedString, accent: bool| {
                            div()
                                .flex_none()
                                .px_1p5()
                                .py_0p5()
                                .rounded_full()
                                .bg(if accent { p.brand_soft } else { badge_bg })
                                .text_xs()
                                .text_color(if accent { p.brand } else { p.text_secondary })
                                .child(text)
                        };
                        let mut hints: Vec<(SharedString, bool)> = Vec::new();
                        if info.names {
                            hints.push((t!("Song title and artist match").into(), true));
                        } else {
                            hints.push((t!("Song title or artist differs").into(), false));
                        }
                        if let Some(delta) = info.duration_delta {
                            hints.push((
                                t!(
                                    "Duration differs by %{seconds} seconds",
                                    seconds = format!("{delta:.1}")
                                )
                                .into(),
                                false,
                            ));
                        }
                        if info.album == Some(false) {
                            hints.push((t!("Different album").into(), false));
                        }
                        if !info.version {
                            hints.push((t!("Different performance version").into(), false));
                        }
                        Button::new(SharedString::from(format!(
                            "lyrics-candidate-{}-{}",
                            record.source.key(),
                            record.id
                        )))
                        .w_full()
                        .h_auto()
                        .justify_start()
                        .py_2p5()
                        .px_3()
                        .rounded(px(tokens::radius::ROW))
                        .border_1()
                        .border_color(if is_selected {
                            p.brand.opacity(if dark { 0.55 } else { 0.45 })
                        } else {
                            hairline
                        })
                        .cursor_pointer()
                        .selected(is_selected)
                        .custom(soft_toggle_style(is_selected, cx))
                        .child(
                            v_flex()
                                .min_w_0()
                                .w_full()
                                .items_start()
                                .gap_1p5()
                                .child(
                                    h_flex()
                                        .w_full()
                                        .min_w_0()
                                        .items_center()
                                        .justify_between()
                                        .gap_2()
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_sm()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(if is_selected {
                                                    p.brand
                                                } else {
                                                    p.text
                                                })
                                                .truncate()
                                                .child(record.track_name.clone()),
                                        )
                                        .child(
                                            div()
                                                .flex_none()
                                                .text_xs()
                                                .font_family(cx.theme().mono_font_family.clone())
                                                .text_color(p.text_muted)
                                                .child(record_duration(record)),
                                        ),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .min_w_0()
                                        .text_xs()
                                        .text_color(p.text_secondary)
                                        .truncate()
                                        .child(if record.album_name.trim().is_empty() {
                                            record.artist_name.clone()
                                        } else {
                                            format!("{} · {}", record.artist_name, record.album_name)
                                        }),
                                )
                                .child(
                                    h_flex()
                                        .flex_wrap()
                                        .items_center()
                                        .gap_1()
                                        .child(tag(record.source.label(), false))
                                        .child(tag(format_label.into(), is_word_timed)),
                                )
                                .child(
                                    h_flex()
                                        .flex_wrap()
                                        .items_center()
                                        .gap_x_2()
                                        .gap_y_0p5()
                                        .children(hints.into_iter().map(|(label, good)| {
                                            h_flex()
                                                .items_center()
                                                .gap_1()
                                                .child(div().size(px(5.0)).rounded_full().bg(
                                                    if good {
                                                        p.online
                                                    } else {
                                                        p.text_muted.opacity(0.6)
                                                    },
                                                ))
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(if good {
                                                            p.text_secondary
                                                        } else {
                                                            p.text_muted
                                                        })
                                                        .child(label),
                                                )
                                        })),
                                ),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.lyrics_ui.selected = Some(index);
                            cx.notify();
                        }))
                    }))
                    .when(ui.request.is_some(), |this| {
                        this.child(
                            v_flex()
                                .flex_1()
                                .min_h(px(120.0))
                                .items_center()
                                .justify_center()
                                .gap_2()
                                .text_sm()
                                .text_color(p.text_secondary)
                                .child(LyricsStatus::SearchingOnline.message()),
                        )
                    })
                    .when_some(ui.error, |this, error| {
                        this.child(
                            div()
                                .px_2()
                                .py_4()
                                .text_sm()
                                .text_color(p.text_secondary)
                                .child(error.message()),
                        )
                    })
                    .when(
                        ui.word_only
                            && ui.searched
                            && ui.request.is_none()
                            && visible_count == 0
                            && ui.error.is_none(),
                        |this| {
                            this.child(
                                div()
                                    .px_2()
                                    .py_4()
                                    .text_sm()
                                    .text_color(p.text_secondary)
                                    .child(t!("No word-by-word lyrics found.")),
                            )
                        },
                    )
                    .children(ui.failures.iter().map(|(id, status)| {
                        div()
                            .px_2()
                            .text_xs()
                            .text_color(p.text_muted)
                            .child(format!("{} · {}", id.label(), status.message()))
                    }))
                    .when(!ui.searched && ui.request.is_none(), |this| {
                        this.child(
                            v_flex()
                                .flex_1()
                                .min_h(px(120.0))
                                .items_center()
                                .justify_center()
                                .px_4()
                                .text_center()
                                .text_sm()
                                .text_color(p.text_muted)
                                .child(t!("Search, then select a candidate to preview.")),
                        )
                    }),
            );

        let selected = ui.selected.and_then(|i| ui.records.get(i));
        let preview = v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .rounded(px(tokens::radius::COVER))
            .bg(panel_bg)
            .border_1()
            .border_color(panel_border)
            .overflow_hidden()
            .child(
                h_flex()
                    .flex_none()
                    .px_3()
                    .py_2()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(hairline)
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child(t!("Preview")),
                    )
                    .when_some(selected, |this, record| {
                        this.child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(p.text_muted)
                                .child(format!(
                                    "{} · {}",
                                    record.source.label(),
                                    record_duration(record)
                                )),
                        )
                    }),
            )
            .child(
                v_flex()
                    .id("lyrics-preview")
                    .flex_1()
                    .min_h_0()
                    .p_3()
                    .overflow_y_scroll()
                    .gap_1p5()
                    .when_some(selected, |this, record| {
                        let parsed = record.lyrics();
                        this.children(parsed.lines.into_iter().map(|line| {
                            let timestamp = line.start_ms.filter(|_| !ui.plain).map(|ms| {
                                format_duration(Some((ms / 1000).min(i32::MAX as u64) as i32))
                            });
                            let translation = line.translation.filter(|t| !t.trim().is_empty());
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .items_start()
                                .gap_2p5()
                                .when_some(timestamp, |row, stamp| {
                                    row.child(
                                        div()
                                            .w(px(42.0))
                                            .flex_none()
                                            .text_right()
                                            .text_xs()
                                            .font_family(cx.theme().mono_font_family.clone())
                                            .text_color(p.text_muted)
                                            .pt(px(2.0))
                                            .child(stamp),
                                    )
                                })
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w_0()
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(p.text)
                                                .line_height(relative(1.4))
                                                .child(line.text),
                                        )
                                        .when_some(translation, |col, trans| {
                                            col.child(
                                                div()
                                                    .text_xs()
                                                    .text_color(p.text_secondary)
                                                    .line_height(relative(1.35))
                                                    .child(trans),
                                            )
                                        }),
                                )
                        }))
                        .when(record.instrumental, |this| {
                            this.child(
                                div()
                                    .py_4()
                                    .text_sm()
                                    .text_color(p.text_secondary)
                                    .child(LyricsStatus::Instrumental.message()),
                            )
                        })
                    })
                    .when(selected.is_none(), |this| {
                        this.child(
                            v_flex()
                                .flex_1()
                                .min_h(px(120.0))
                                .items_center()
                                .justify_center()
                                .px_4()
                                .text_center()
                                .text_sm()
                                .text_color(p.text_muted)
                                .child(t!("Search, then select a candidate to preview.")),
                        )
                    }),
            );

        let body_height = (viewport_height - 340.0).clamp(180.0, 440.0);
        let results = if compact {
            v_flex()
                .h(px(body_height))
                .gap_3()
                .child(candidates)
                .child(preview)
                .into_any_element()
        } else {
            h_flex()
                .h(px(body_height))
                .items_stretch()
                .gap_3()
                .child(candidates)
                .child(preview)
                .into_any_element()
        };
        let uncertain = selected.is_some_and(|r| !matching::assess(&ui.query, r).trusted);
        let content = v_flex()
            .min_w_0()
            .gap_3()
            .child(fields)
            .when(!enabled, |this| {
                this.child(
                    h_flex()
                        .w_full()
                        .px_3()
                        .py_2p5()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .rounded(px(tokens::radius::ROW))
                        .bg(p.brand_soft)
                        .border_1()
                        .border_color(p.brand.opacity(0.28))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.text)
                                        .child(t!(
                                            "Enable online lyrics in Settings to search lyrics services."
                                        )),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(p.text_secondary)
                                        .child(t!(
                                            "When enabled, song metadata is sent to the selected lyrics services."
                                        )),
                                ),
                        )
                        .child(
                            surface_button("enable-online-lyrics")
                                .label(t!("Enable online lyrics"))
                                .compact_action()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.save_lyrics_settings(|s| s.enabled = true, cx);
                                })),
                        ),
                )
            })
            .child(results)
            .when(uncertain, |this| {
                this.child(
                    h_flex()
                        .w_full()
                        .px_3()
                        .py_2()
                        .items_center()
                        .gap_2()
                        .rounded(px(tokens::radius::CONTROL))
                        .bg(panel_bg)
                        .border_1()
                        .border_color(hairline)
                        .child(div().size(px(6.0)).flex_none().rounded_full().bg(p.brand))
                        .child(
                            div()
                                .text_xs()
                                .text_color(p.text_secondary)
                                .child(t!(
                                    "Timing may differ for this version. You can display it as plain text."
                                )),
                        ),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .px_3()
                    .py_2()
                    .gap_5()
                    .flex_wrap()
                    .items_center()
                    .rounded(px(tokens::radius::CONTROL))
                    .bg(panel_bg)
                    .border_1()
                    .border_color(hairline)
                    .child(
                        Switch::new("remember-lyrics-choice")
                            .checked(ui.remember)
                            .label(t!("Remember this song's choice").to_string())
                            .on_change(cx.listener(|this, checked, _, cx| {
                                this.lyrics_ui.remember = *checked;
                                cx.notify();
                            })),
                    )
                    .child(
                        Switch::new("lyrics-candidate-plain")
                            .checked(ui.plain)
                            .label(t!("Display as plain text").to_string())
                            .on_change(cx.listener(|this, checked, _, cx| {
                                this.lyrics_ui.plain = *checked;
                                cx.notify();
                            })),
                    ),
            );
        dialog
            .title(t!("Change lyrics for “%{title}”", title = ui.song_title))
            .w(px(if compact { 640.0 } else { 940.0 }))
            .max_w(px(f32::from(window.viewport_size().width) - 32.0))
            .margin_top(px(16.0))
            .child(content)
            .footer(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        ghost_button("cancel-lyrics-search")
                            .label(t!("Cancel"))
                            .dialog_action()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_lyrics_dialog(window, cx)
                            })),
                    )
                    .child(
                        primary_button("apply-lyrics-candidate")
                            .label(t!("Use these lyrics"))
                            .dialog_action()
                            .disabled(selected.is_none() || ui.request.is_some())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.apply_lyrics_candidate(window, cx)
                            })),
                    ),
            )
    }
}
