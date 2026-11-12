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
        let document = self.state.lyrics.document.as_ref();
        let synced = self.state.lyrics.lyrics().is_some_and(|l| l.is_synced());
        let original_synced = document.is_some_and(|d| d.lyrics.is_synced());
        let plain = document.is_some_and(|d| d.plain);
        let personal = document.is_some_and(|d| d.manual || d.offset_ms != 0 || d.plain);
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
        v_flex()
            .gap_1()
            .pb_2()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_base()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(t!("Lyrics")),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(
                                Button::new("change-lyrics")
                                    .label(t!("Change lyrics"))
                                    .ghost()
                                    .small()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_lyrics_dialog(LyricsDialog::Search, window, cx)
                                    })),
                            )
                            .child(
                                Button::new("lyrics-actions")
                                    .label("⋯")
                                    .accessibility_label(t!("Lyrics actions"))
                                    .ghost()
                                    .small()
                                    .dropdown_menu(move |menu, _, cx| {
                                        app.update(cx, |_, cx| {
                                            let mut menu = menu
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
                                                        .disabled(!synced)
                                                        .on_click(cx.listener(
                                                            |this, _, window, cx| {
                                                                this.open_lyrics_dialog(
                                                                    LyricsDialog::Timing,
                                                                    window,
                                                                    cx,
                                                                )
                                                            },
                                                        )),
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
                                    }),
                            ),
                    ),
            )
            .when_some(source, |this, source| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(palette(cx).text_secondary)
                        .child(source),
                )
            })
            .when_some(self.state.lyrics.warning, |this, status| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(palette(cx).text_secondary)
                        .child(status.message()),
                )
            })
            .into_any_element()
    }

    pub(in crate::app) fn render_lyrics_dialog(
        &self,
        dialog: Dialog,
        window: &Window,
        cx: &Context<Self>,
    ) -> Dialog {
        if self.lyrics_ui.dialog == Some(LyricsDialog::Timing) {
            return self.render_lyrics_timing_dialog(dialog, cx);
        }
        let compact = f32::from(window.viewport_size().width) < 820.0;
        let viewport_height = f32::from(window.viewport_size().height);
        let ui = &self.lyrics_ui;
        let enabled = self.config.lyrics.enabled;
        let app = cx.entity().downgrade();
        let source = ui.source;
        let sources = self.config.lyrics.enabled_sources();
        let fields = v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(div().text_xs().child(t!("Song title")))
                            .child(Input::new(&ui.title)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(div().text_xs().child(t!("Artist (optional)")))
                            .child(Input::new(&ui.artist)),
                    ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_end()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(div().text_xs().child(t!("Album (optional)")))
                            .child(Input::new(&ui.album)),
                    )
                    .child(
                        Button::new("search-online-lyrics")
                            .label(t!("Search lyrics"))
                            .primary()
                            .disabled(!enabled)
                            .on_click(cx.listener(|this, _, _, cx| this.search_lyrics(cx))),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().text_xs().child(t!("Lyrics source")))
                    .child(
                        Button::new("lyrics-search-source")
                            .label(
                                source
                                    .map(ProviderId::label)
                                    .unwrap_or_else(|| t!("All enabled sources").into()),
                            )
                            .outline()
                            .small()
                            .dropdown_caret(true)
                            .disabled(!enabled || sources.is_empty())
                            .dropdown_menu(move |menu, _, cx| {
                                app.update(cx, |_, cx| {
                                    std::iter::once(None)
                                        .chain(sources.iter().copied().map(Some))
                                        .fold(menu, |menu, choice| {
                                            menu.item(
                                                PopupMenuItem::new(
                                                    choice.map(ProviderId::label).unwrap_or_else(
                                                        || t!("All enabled sources").into(),
                                                    ),
                                                )
                                                .checked(choice == source)
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.lyrics_ui.source = choice;
                                                    this.search_lyrics(cx);
                                                })),
                                            )
                                        })
                                })
                                .expect("app owns lyrics source picker")
                            }),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(palette(cx).text_secondary)
                    .child(
                        ui.query
                            .duration
                            .map(|d| {
                                t!(
                                    "Current audio: %{duration}",
                                    duration = format_duration(Some(d as i32))
                                )
                                .to_string()
                            })
                            .unwrap_or_else(|| t!("Unknown duration").to_string()),
                    ),
            );
        let candidates = v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t!("Candidates")),
            )
            .child(
                v_flex()
                    .id("lyrics-candidates")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&ui.candidates_scroll)
                    .gap_2()
                    .children(ui.records.iter().enumerate().map(|(index, record)| {
                        let info = matching::assess(&ui.query, record);
                        Button::new(SharedString::from(format!(
                            "lyrics-candidate-{}-{}",
                            record.source.key(),
                            record.id
                        )))
                        .w_full()
                        .h_auto()
                        .justify_start()
                        .outline()
                        .selected(ui.selected == Some(index))
                        .custom(soft_toggle_style(ui.selected == Some(index), cx))
                        .child(
                            v_flex()
                                .min_w_0()
                                .w_full()
                                .items_start()
                                .gap_1()
                                .child(div().text_sm().truncate().child(format!(
                                    "{} · {}",
                                    record.track_name, record.artist_name
                                )))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(palette(cx).text_secondary)
                                        .child(format!(
                                            "{} · {} · {} · {}",
                                            record.source.label(),
                                            record.album_name,
                                            record_duration(record),
                                            if record.instrumental {
                                                t!("Instrumental")
                                            } else if record.lyrics().is_synced() {
                                                t!("Synced lyrics")
                                            } else {
                                                t!("Plain text lyrics")
                                            }
                                        )),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(palette(cx).text_secondary)
                                        .child(if info.names {
                                            t!("Song title and artist match")
                                        } else {
                                            t!("Song title or artist differs")
                                        }),
                                )
                                .when_some(info.duration_delta, |this, delta| {
                                    this.child(
                                        div()
                                            .text_xs()
                                            .text_color(palette(cx).text_secondary)
                                            .child(t!(
                                                "Duration differs by %{seconds} seconds",
                                                seconds = format!("{delta:.1}")
                                            )),
                                    )
                                })
                                .when(info.album == Some(false), |this| {
                                    this.child(
                                        div()
                                            .text_xs()
                                            .text_color(palette(cx).text_secondary)
                                            .child(t!("Different album")),
                                    )
                                })
                                .when(!info.version, |this| {
                                    this.child(
                                        div()
                                            .text_xs()
                                            .text_color(palette(cx).text_secondary)
                                            .child(t!("Different performance version")),
                                    )
                                }),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.lyrics_ui.selected = Some(index);
                            cx.notify();
                        }))
                    }))
                    .when(ui.request.is_some(), |this| {
                        this.child(
                            div()
                                .text_sm()
                                .child(LyricsStatus::SearchingOnline.message()),
                        )
                    })
                    .when_some(ui.error, |this, error| {
                        this.child(div().text_sm().child(error.message()))
                    })
                    .children(ui.failures.iter().map(|(id, status)| {
                        div()
                            .text_xs()
                            .text_color(palette(cx).text_secondary)
                            .child(format!("{} · {}", id.label(), status.message()))
                    }))
                    .when(!ui.searched, |this| {
                        this.child(
                            div()
                                .text_sm()
                                .text_color(palette(cx).text_secondary)
                                .child(t!("Search, then select a candidate to preview.")),
                        )
                    }),
            );
        let selected = ui.selected.and_then(|i| ui.records.get(i));
        let preview = v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .gap_2()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(t!("Preview")),
            )
            .child(
                v_flex()
                    .id("lyrics-preview")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap_2()
                    .when_some(selected, |this, record| {
                        this.children(record.lyrics().lines.into_iter().map(|line| {
                            div().text_sm().child(
                                line.start_ms
                                    .filter(|_| !ui.plain)
                                    .map(|ms| {
                                        format!(
                                            "[{}] {}",
                                            format_duration(Some(
                                                (ms / 1000).min(i32::MAX as u64) as i32
                                            )),
                                            line.text
                                        )
                                    })
                                    .unwrap_or(line.text),
                            )
                        }))
                        .when(record.instrumental, |this| {
                            this.child(LyricsStatus::Instrumental.message())
                        })
                    }),
            );
        let body_height = (viewport_height - 380.0).clamp(120.0, 400.0);
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
                .gap_4()
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
                this.child(div().text_sm().child(t!(
                    "Enable online lyrics in Settings to search lyrics services."
                )))
                .child(
                    div()
                        .text_xs()
                        .text_color(palette(cx).text_secondary)
                        .child(t!(
                            "When enabled, song metadata is sent to the selected lyrics services."
                        )),
                )
                .child(
                    Button::new("enable-online-lyrics")
                        .label(t!("Enable online lyrics"))
                        .outline()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.save_lyrics_settings(|s| s.enabled = true, cx);
                        })),
                )
            })
            .child(results)
            .when(uncertain, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(palette(cx).text_secondary)
                        .child(t!(
                            "Timing may differ for this version. You can display it as plain text."
                        )),
                )
            })
            .child(
                h_flex()
                    .gap_4()
                    .flex_wrap()
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
            .w(px(if compact { 620.0 } else { 900.0 }))
            .max_w(px(f32::from(window.viewport_size().width) - 32.0))
            .margin_top(px(16.0))
            .child(content)
            .footer(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("cancel-lyrics-search")
                            .label(t!("Cancel"))
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_lyrics_dialog(window, cx)
                            })),
                    )
                    .child(
                        Button::new("apply-lyrics-candidate")
                            .label(t!("Use these lyrics"))
                            .primary()
                            .disabled(selected.is_none() || ui.request.is_some())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.apply_lyrics_candidate(window, cx)
                            })),
                    ),
            )
    }

    fn render_lyrics_timing_dialog(&self, dialog: Dialog, cx: &Context<Self>) -> Dialog {
        let offset = self
            .state
            .lyrics
            .document
            .as_ref()
            .map_or(0, |d| d.offset_ms);
        dialog.title(t!("Lyrics timing")).w(px(480.0))
            .child(v_flex().gap_3()
                .child(h_flex().gap_2().items_center().justify_between()
                    .child(Button::new("lyrics-earlier").label(t!("Earlier 0.5 s")).outline().on_click(cx.listener(|this,_,window,cx| { this.adjust_lyrics_offset(-500,window,cx); })))
                    .child(div().text_sm().child(t!("Offset: %{seconds} s",seconds=format!("{:+.1}",offset as f64/1000.0))))
                    .child(Button::new("lyrics-later").label(t!("Later 0.5 s")).outline().on_click(cx.listener(|this,_,window,cx| { this.adjust_lyrics_offset(500,window,cx); }))))
                .child(div().text_xs().text_color(palette(cx).text_secondary).child(t!("Positive values delay lyrics; negative values show them earlier. Only this song is affected.")))
                .child(h_flex().gap_2().items_center().child(Input::new(&self.lyrics_ui.offset).flex_1()).child(div().text_sm().child(t!("ms")))
                    .child(Button::new("apply-lyrics-offset").label(t!("Apply")).primary().on_click(cx.listener(|this,_,_,cx| this.apply_lyrics_offset_input(cx))))
                    .child(Button::new("reset-lyrics-offset").label(t!("Reset")).ghost().on_click(cx.listener(|this,_,window,cx| {
                        this.change_lyrics_display(|d| d.offset_ms=0,cx);
                        let offset=this.state.lyrics.document.as_ref().map_or(0,|d| d.offset_ms);
                        this.lyrics_ui.offset.update(cx,|i,cx| i.set_value(offset.to_string(),window,cx));
                    })))))
            .footer(Button::new("close-lyrics-timing").label(t!("Done")).on_click(cx.listener(|this,_,window,cx| this.close_lyrics_dialog(window,cx))))
    }

    fn adjust_lyrics_offset(&mut self, delta: i64, window: &mut Window, cx: &mut Context<Self>) {
        self.change_lyrics_display(|d| d.offset_ms = d.offset_ms.saturating_add(delta), cx);
        let offset = self
            .state
            .lyrics
            .document
            .as_ref()
            .map_or(0, |d| d.offset_ms);
        self.lyrics_ui
            .offset
            .update(cx, |i, cx| i.set_value(offset.to_string(), window, cx));
    }
}
