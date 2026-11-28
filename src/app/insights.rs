//! Local privacy-first Listening Insights view (KPIs, 24h rhythm, top songs/artists/albums/genres).

use std::time::{SystemTime, UNIX_EPOCH};

use crate::app::prelude::*;
use crate::listening_stats::{
    aggregate_insights, ListeningInsights, ListeningRange, RankedStat,
};

#[derive(Default)]
pub(in crate::app) struct InsightsState {
    pub(in crate::app) range: ListeningRange,
    cached: Option<ListeningInsights>,
    dirty: bool,
}

impl InsightsState {
    pub(in crate::app) fn set_range(&mut self, range: ListeningRange) {
        if self.range != range {
            self.range = range;
            self.dirty = true;
        }
    }

    pub(in crate::app) fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub(in crate::app) fn needs_refresh(&self) -> bool {
        self.dirty || self.cached.is_none()
    }

    pub(in crate::app) fn set_snapshot(&mut self, snapshot: ListeningInsights) {
        self.cached = Some(snapshot);
        self.dirty = false;
    }

    pub(in crate::app) fn snapshot(&self) -> Option<&ListeningInsights> {
        self.cached.as_ref()
    }
}

impl NavidromeApp {
    pub(in crate::app) fn refresh_listening_insights(&mut self) {
        let now_ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let entries = self
            .listening_log
            .merged_entries(&self.state.listening.history().events);
        let snapshot = aggregate_insights(&entries, self.state.insights.range, now_ts);
        for item in snapshot.top_songs.iter().take(20) {
            self.ensure_song_cover(&item.song, false);
        }
        self.state.insights.set_snapshot(snapshot);
    }

    pub(in crate::app) fn set_insights_range(&mut self, range: ListeningRange) {
        self.state.insights.set_range(range);
        self.refresh_listening_insights();
    }

    pub(in crate::app) fn record_finished_listening_event(&mut self) {
        if let Some(latest) = self.state.listening.history().events.last() {
            match self.listening_log.record_event(latest) {
                Ok(true) => {
                    self.state.insights.mark_dirty();
                }
                Ok(false) => {}
                Err(error) => {
                    log::warn!("failed to append listening log: {error:#}");
                }
            }
        }
    }

    pub(in crate::app) fn render_insights(
        &self,
        _window: &mut Window,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let p = palette(cx);
        let dark = cx.theme().is_dark();
        let card_bg = if dark {
            hsla(0.0, 0.0, 1.0, 0.035)
        } else {
            p.sunken.opacity(0.65)
        };
        let card_border = if dark {
            hsla(0.0, 0.0, 1.0, 0.08)
        } else {
            p.border.opacity(0.75)
        };

        let active_range = self.state.insights.range;
        let snapshot = self.state.insights.snapshot();

        let range_bar = h_flex()
            .items_center()
            .gap_1p5()
            .flex_wrap()
            .children(ListeningRange::ALL.into_iter().map(|range| {
                let selected = range == active_range;
                let label = range_label(range);
                let button = if selected {
                    primary_button(SharedString::from(format!("insights-range-{}", range.id())))
                } else {
                    surface_button(SharedString::from(format!("insights-range-{}", range.id())))
                };
                button
                    .label(label)
                    .compact_action()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_insights_range(range);
                        cx.notify();
                    }))
            }));

        let header = h_flex()
            .w_full()
            .items_end()
            .justify_between()
            .gap_3()
            .flex_wrap()
            .child(div().flex_1().min_w(px(240.0)).child(self.page_header(
                t!("Listening Insights"),
                t!("Stored locally on your device · never uploaded"),
                cx,
            )))
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .flex_wrap()
                    .child(range_bar)
                    .child(self.header_refresh_button(
                        "refresh-insights",
                        t!("Refresh insights"),
                        false,
                        cx,
                        |this, _cx| {
                            this.listening_log.reload();
                            this.refresh_listening_insights();
                        },
                    )),
            );

        let Some(stats) = snapshot.filter(|s| s.play_count > 0) else {
            return v_flex()
                .gap_6()
                .child(header)
                .child(
                    v_flex()
                        .w_full()
                        .min_h(px(220.0))
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .px_6()
                        .rounded(px(tokens::radius::LARGE))
                        .bg(card_bg)
                        .border_1()
                        .border_color(card_border)
                        .text_center()
                        .child(
                            Icon::new(AppIcon::Insights)
                                .with_size(px(30.0))
                                .text_color(p.text_muted),
                        )
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(p.text)
                                .child(t!("No listening stats for this period yet")),
                        )
                        .child(
                            div()
                                .max_w(px(420.0))
                                .text_sm()
                                .text_color(p.text_secondary)
                                .child(t!(
                                    "Play tracks for at least 30 seconds (or finish them) to build your local listening summary."
                                )),
                        ),
                )
                .into_any_element();
        };

        // Top KPI cards
        let kpis = h_flex()
            .w_full()
            .gap_3()
            .flex_wrap()
            .child(render_kpi_card(
                t!("Total listening time").to_string(),
                format_listening_duration(stats.total_listened_secs),
                t!(
                    "Avg %{avg}/day · %{days} active days",
                    avg = format_listening_duration(stats.avg_daily_secs),
                    days = stats.active_days
                )
                .to_string(),
                card_bg,
                card_border,
                cx,
            ))
            .child(render_kpi_card(
                t!("Plays").to_string(),
                stats.play_count.to_string(),
                t!("%{count} completed", count = stats.completed_count).to_string(),
                card_bg,
                card_border,
                cx,
            ))
            .child(render_kpi_card(
                t!("Unique tracks").to_string(),
                stats.unique_songs.to_string(),
                t!(
                    "%{artists} artists · %{albums} albums",
                    artists = stats.unique_artists,
                    albums = stats.unique_albums
                )
                .to_string(),
                card_bg,
                card_border,
                cx,
            ))
            .child(render_kpi_card(
                t!("Peak listening hour").to_string(),
                stats
                    .peak_hour
                    .map(|h| format!("{h:02}:00"))
                    .unwrap_or_else(|| "—".to_string()),
                t!("Local time").to_string(),
                card_bg,
                card_border,
                cx,
            ));

        // 24-hour rhythm chart
        let max_hour_secs = stats.hourly_secs.iter().copied().max().unwrap_or(1).max(1);
        let hourly_chart = v_flex()
            .w_full()
            .p_4()
            .gap_3()
            .rounded(px(tokens::radius::LARGE))
            .bg(card_bg)
            .border_1()
            .border_color(card_border)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(tokens::BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child(t!("Listening rhythm (24h)")),
                    )
                    .child(
                        div()
                            .text_size(px(tokens::CAPTION))
                            .text_color(p.text_muted)
                            .child(t!("Time spent by hour of day")),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .h(px(88.0))
                    .items_end()
                    .gap_1()
                    .children((0..24).map(|hour| {
                        let secs = stats.hourly_secs[hour];
                        let ratio = (secs as f32 / max_hour_secs as f32).clamp(0.0, 1.0);
                        let height = if secs == 0 {
                            4.0
                        } else {
                            (ratio * 66.0).max(8.0)
                        };
                        let is_peak = stats.peak_hour == Some(hour);
                        let tip = format!(
                            "{hour:02}:00 – {:02}:00 · {}",
                            (hour + 1) % 24,
                            format_listening_duration(secs)
                        );
                        v_flex()
                            .id(SharedString::from(format!("insights-hour-{hour}")))
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .justify_end()
                            .items_center()
                            .gap_1()
                            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                            .child(
                                div()
                                    .w_full()
                                    .h(px(height))
                                    .rounded_t(px(4.0))
                                    .bg(if is_peak {
                                        p.brand
                                    } else if secs > 0 {
                                        p.brand.opacity(0.45)
                                    } else {
                                        p.border.opacity(0.45)
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(10.0))
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .text_color(p.text_muted)
                                    .child(if hour % 6 == 0 {
                                        format!("{hour:02}")
                                    } else {
                                        String::new()
                                    }),
                            )
                    })),
            );

        // Top 20 tracks section with one-click queue generation
        let top_songs_vec: Vec<Song> = stats.top_songs.iter().map(|s| s.song.clone()).collect();
        let top_songs_for_play = top_songs_vec.clone();
        let top_songs_section = v_flex()
            .w_full()
            .p_4()
            .gap_3()
            .rounded(px(tokens::radius::LARGE))
            .bg(card_bg)
            .border_1()
            .border_color(card_border)
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(tokens::BODY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(p.text)
                            .child(t!("Top tracks")),
                    )
                    .child(
                        capsule_button_sm("play-top-tracks")
                            .icon(PlayerIcon::Play)
                            .label(t!("Play top tracks"))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.replace_queue_and_play_mixed(
                                    top_songs_for_play.clone(),
                                    0,
                                    QueueOrigin::Manual,
                                );
                                cx.notify();
                            })),
                    ),
            )
            .child(v_flex().gap_1().children(stats.top_songs.iter().enumerate().map(
                |(idx, item)| {
                    let all_songs = top_songs_vec.clone();
                    let cover = self.render_song_cover_styled(
                        &item.song,
                        36.0,
                        tokens::radius::CONTROL,
                        Vec::new(),
                    );
                    let artist_album = if item.song.album.is_empty() {
                        item.song.artist.clone()
                    } else if item.song.artist.is_empty() {
                        item.song.album.clone()
                    } else {
                        format!("{} · {}", item.song.artist, item.song.album)
                    };
                    h_flex()
                        .id(SharedString::from(format!("insights-top-song-{idx}")))
                        .w_full()
                        .px_2()
                        .py_1p5()
                        .gap_3()
                        .items_center()
                        .rounded(px(tokens::radius::ROW))
                        .cursor_pointer()
                        .hover(|style| style.bg(p.sunken))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.replace_queue_and_play_mixed(
                                all_songs.clone(),
                                idx,
                                QueueOrigin::Manual,
                            );
                            cx.notify();
                        }))
                        .child(
                            div()
                                .w(px(24.0))
                                .text_right()
                                .text_size(px(tokens::CAPTION))
                                .font_family(cx.theme().mono_font_family.clone())
                                .text_color(if idx < 3 { p.brand } else { p.text_muted })
                                .child(format!("{}", idx + 1)),
                        )
                        .child(cover)
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .text_size(px(tokens::BODY))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.text)
                                        .truncate()
                                        .child(item.song.title.clone()),
                                )
                                .when(!artist_album.is_empty(), |this| {
                                    this.child(
                                        div()
                                            .text_size(px(tokens::CAPTION))
                                            .text_color(p.text_secondary)
                                            .truncate()
                                            .child(artist_album),
                                    )
                                }),
                        )
                        .child(
                            v_flex()
                                .flex_none()
                                .items_end()
                                .child(
                                    div()
                                        .text_size(px(tokens::META))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(p.text)
                                        .child(t!("%{count} plays", count = item.play_count)),
                                )
                                .child(
                                    div()
                                        .text_size(px(tokens::CAPTION))
                                        .text_color(p.text_muted)
                                        .child(format_listening_duration(item.listened_secs)),
                                ),
                        )
                },
            )));

        // Ranked cards: Top Artists, Top Albums, Top Genres
        let rankings = h_flex()
            .w_full()
            .gap_3()
            .flex_wrap()
            .items_start()
            .when(!stats.top_artists.is_empty(), |this| {
                this.child(render_ranked_card(
                    t!("Top artists").to_string(),
                    &stats.top_artists,
                    card_bg,
                    card_border,
                    cx,
                ))
            })
            .when(!stats.top_albums.is_empty(), |this| {
                this.child(render_ranked_card(
                    t!("Top albums").to_string(),
                    &stats.top_albums,
                    card_bg,
                    card_border,
                    cx,
                ))
            })
            .when(!stats.top_genres.is_empty(), |this| {
                this.child(render_ranked_card(
                    t!("Top genres").to_string(),
                    &stats.top_genres,
                    card_bg,
                    card_border,
                    cx,
                ))
            });

        v_flex()
            .gap_5()
            .child(header)
            .child(kpis)
            .child(hourly_chart)
            .child(rankings)
            .child(top_songs_section)
            .into_any_element()
    }
}

fn range_label(range: ListeningRange) -> String {
    match range {
        ListeningRange::Last7Days => t!("Last 7 days").to_string(),
        ListeningRange::Last30Days => t!("Last 30 days").to_string(),
        ListeningRange::ThisYear => t!("This year").to_string(),
        ListeningRange::AllTime => t!("All time").to_string(),
    }
}

fn render_kpi_card(
    label: String,
    value: String,
    subtitle: String,
    bg: Hsla,
    border: Hsla,
    cx: &App,
) -> gpui_kit::AnyElement {
    let p = palette(cx);
    v_flex()
        .flex_1()
        .min_w(px(170.0))
        .p_4()
        .gap_1()
        .rounded(px(tokens::radius::LARGE))
        .bg(bg)
        .border_1()
        .border_color(border)
        .child(
            div()
                .text_size(px(tokens::CAPTION))
                .text_color(p.text_muted)
                .child(label),
        )
        .child(
            div()
                .text_size(px(tokens::PAGE_TITLE))
                .font_weight(FontWeight::BOLD)
                .text_color(p.text)
                .child(value),
        )
        .child(
            div()
                .text_size(px(tokens::CAPTION))
                .text_color(p.text_secondary)
                .truncate()
                .child(subtitle),
        )
        .into_any_element()
}

fn render_ranked_card(
    title: String,
    items: &[RankedStat],
    bg: Hsla,
    border: Hsla,
    cx: &App,
) -> gpui_kit::AnyElement {
    let p = palette(cx);
    let max_secs = items.iter().map(|i| i.listened_secs).max().unwrap_or(1).max(1);
    v_flex()
        .flex_1()
        .min_w(px(240.0))
        .p_4()
        .gap_3()
        .rounded(px(tokens::radius::LARGE))
        .bg(bg)
        .border_1()
        .border_color(border)
        .child(
            div()
                .text_size(px(tokens::BODY))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(p.text)
                .child(title),
        )
        .child(v_flex().gap_2p5().children(items.iter().map(|item| {
            let pct = ((item.listened_secs as f32 / max_secs as f32) * 100.0).clamp(4.0, 100.0);
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .w_full()
                        .items_baseline()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(tokens::META))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(p.text)
                                .truncate()
                                .child(item.name.clone()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(tokens::CAPTION))
                                .text_color(p.text_muted)
                                .child(format!(
                                    "{} · {}",
                                    t!("%{count} plays", count = item.play_count),
                                    format_listening_duration(item.listened_secs)
                                )),
                        ),
                )
                .when_some(item.subtitle.as_ref(), |this, subtitle| {
                    this.child(
                        div()
                            .text_size(px(tokens::CAPTION))
                            .text_color(p.text_secondary)
                            .truncate()
                            .child(subtitle.clone()),
                    )
                })
                .child(
                    div()
                        .w_full()
                        .h(px(5.0))
                        .rounded_full()
                        .bg(p.border.opacity(0.4))
                        .child(
                            div()
                                .h_full()
                                .w(gpui_kit::relative(pct / 100.0))
                                .rounded_full()
                                .bg(p.brand.opacity(0.75)),
                        ),
                )
        })))
        .into_any_element()
}

pub(in crate::app) fn format_listening_duration(secs: u64) -> String {
    if secs < 60 {
        return t!("%{secs}s", secs = secs).to_string();
    }
    let total_mins = secs / 60;
    let hours = total_mins / 60;
    let mins = total_mins % 60;
    if hours == 0 {
        t!("%{mins}m", mins = mins).to_string()
    } else if mins == 0 {
        t!("%{hours}h", hours = hours).to_string()
    } else {
        t!("%{hours}h %{mins}m", hours = hours, mins = mins).to_string()
    }
}
