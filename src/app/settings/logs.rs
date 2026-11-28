//! Read-only log viewer. File I/O runs off the UI thread only while visible.

use std::time::Instant;

use chrono::{DateTime, Local};
use gpui_kit::component::input::{Textarea, TextareaState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::ClipboardItem;

use super::components::{settings_card, settings_divider, settings_group, settings_row};
use crate::app::playback::display::format_file_size;
use crate::app::prelude::*;
use crate::diagnostics::{sanitize_urls, LogFilter, LogTail, MAX_LOG_ENTRIES};
use crate::memory_stats::{AppMemoryBreakdown, MemorySnapshot};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);

pub(in crate::app) struct LogViewer {
    search: Entity<InputState>,
    text: Entity<TextareaState>,
    tail: LogTail,
    filter: LogFilter,
    auto_refresh: bool,
    dirty: bool,
    last_read: Option<Instant>,
    pending: Option<Receiver<std::io::Result<LogTail>>>,
    updated_at: Option<DateTime<Local>>,
    error: Option<String>,
    file_error: Option<String>,
    rendered: String,
    matching: usize,
    playback: PlaybackState,
    memory: MemorySnapshot,
}

impl LogViewer {
    pub(in crate::app) fn new(window: &mut Window, cx: &mut Context<NavidromeApp>) -> Self {
        Self {
            search: cx.new(|cx| InputState::new(window, cx).placeholder(t!("Search logs"))),
            text: cx.new(|cx| TextareaState::new(window, cx).soft_wrap(false)),
            tail: LogTail::default(),
            filter: LogFilter::All,
            auto_refresh: true,
            dirty: false,
            last_read: None,
            pending: None,
            updated_at: None,
            error: None,
            file_error: None,
            rendered: String::new(),
            matching: 0,
            playback: PlaybackState::default(),
            memory: MemorySnapshot::default(),
        }
    }

    pub(in crate::app) fn refresh(&mut self) {
        self.last_read = None;
    }
}

impl LogFilter {
    fn label(self) -> SharedString {
        match self {
            Self::All => t!("All logs"),
            Self::WarningsAndErrors => t!("Warnings and errors"),
            Self::Playback => t!("Playback logs"),
        }
        .into()
    }
}

impl NavidromeApp {
    fn set_file_logging(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let previous = self.config.log_to_file;
        if let Err(error) = crate::logging::set_file_enabled(enabled) {
            self.notify_error(t!("Could not change file logging: %{error}", error = error));
            self.logs.refresh();
            cx.notify();
            return;
        }
        let mut next = self.config.clone();
        next.log_to_file = enabled;
        match config::save(&next) {
            Ok(()) => {
                self.config = next;
                log::info!("file logging changed; enabled={enabled}");
            }
            Err(error) => {
                let _ = crate::logging::set_file_enabled(previous);
                self.notify_error(t!("Settings save failed: %{error}", error = error));
            }
        }
        self.logs.refresh();
        cx.notify();
    }

    pub(in crate::app) fn subscribe_log_search(&mut self, cx: &mut Context<Self>) {
        let input = self.logs.search.clone();
        self._subscriptions
            .push(cx.subscribe(&input, |this, _, event, _| {
                if matches!(event, InputEvent::Change) {
                    this.logs.dirty = true;
                }
            }));
    }

    pub(in crate::app) fn poll_logs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !window.is_visible()
            || !self.state.settings.open
            || self.state.settings.section != SettingsSection::Logs
        {
            return false;
        }
        let mut changed = false;
        let result = self
            .logs
            .pending
            .as_ref()
            .and_then(|rx| match rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err(std::io::Error::other("log reader stopped")))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some(result) = result {
            self.logs.pending = None;
            match result {
                Ok(tail) => {
                    self.logs.tail = tail;
                    self.logs.error = None;
                    self.logs.updated_at = Some(Local::now());
                    self.logs.playback = self.audio.state();
                    self.logs.memory = self.sample_memory_snapshot();
                    self.logs.file_error = crate::logging::file_error();
                    self.logs.dirty = true;
                }
                Err(error) => self.logs.error = Some(error.to_string()),
            }
            changed = true;
        }
        if self.logs.dirty {
            let search = self.logs.search.read(cx).value();
            let (text, count) = self.logs.tail.filtered_text(self.logs.filter, &search);
            self.logs.matching = count;
            if self.logs.rendered != text {
                self.logs
                    .text
                    .update(cx, |state, cx| state.set_value(text.clone(), window, cx));
                self.logs.rendered = text;
            }
            self.logs.dirty = false;
            changed = true;
        }
        let now = Instant::now();
        let due = self.logs.last_read.is_none()
            || (self.logs.auto_refresh
                && self
                    .logs
                    .last_read
                    .is_some_and(|last| now.duration_since(last) >= REFRESH_INTERVAL));
        if due && self.logs.pending.is_none() {
            let (tx, rx) = mpsc::channel();
            self.runtime.spawn_blocking(move || {
                let _ = tx.send(Ok(crate::logging::snapshot()));
            });
            self.logs.pending = Some(rx);
            self.logs.last_read = Some(now);
            changed = true;
        }
        changed
    }

    fn sample_memory_snapshot(&self) -> MemorySnapshot {
        let (cover_source_count, cover_source_bytes) = self.state.covers.stats();
        let (cover_texture_count, cover_texture_capacity) = self.cover_textures.borrow().stats();
        let (cover_thumbnail_count, cover_thumbnail_bytes) = self.cover_thumbnails.borrow().stats();
        let default_cover_bytes = self.default_cover_thumbnails.borrow().memory_bytes();
        let vinyl_bytes = self.vinyl_cover.memory_bytes();
        let (lyrics_bodies, lyrics_bytes) = self.lyrics_cache.stats();
        let playlist_songs = self.state.playlists.songs().len();
        let playlist_search_bytes = self.state.playlists.search.index_bytes();
        let queue_entries = self.state.queue.len();
        let library_items =
            self.state.artists.len() + self.state.albums.len() + self.state.playlists.items().len();
        let log_buffer_bytes = crate::logging::memory_bytes();

        MemorySnapshot {
            process: crate::memory_stats::sample_process_memory(),
            app: AppMemoryBreakdown {
                cover_source_count,
                cover_source_bytes: cover_source_bytes as u64,
                cover_texture_count,
                cover_texture_capacity,
                cover_thumbnail_count,
                cover_thumbnail_bytes: cover_thumbnail_bytes as u64,
                default_cover_bytes: default_cover_bytes as u64,
                vinyl_bytes: vinyl_bytes as u64,
                lyrics_bodies,
                lyrics_bytes: lyrics_bytes as u64,
                playlist_songs,
                playlist_search_bytes: playlist_search_bytes as u64,
                queue_entries,
                library_items,
                log_buffer_bytes: log_buffer_bytes as u64,
            },
        }
    }

    fn log_memory_details(&self) -> Vec<String> {
        let snap = &self.logs.memory;
        let app = &snap.app;
        let mut lines = Vec::new();
        if let Some(proc_mem) = snap.process {
            let mut summary = t!(
                "Process memory (RSS): %{rss}",
                rss = format_file_size(proc_mem.rss_bytes)
            )
            .to_string();
            if let Some(peak) = proc_mem.peak_rss_bytes {
                summary.push_str(&format!(
                    " · {}",
                    t!("Peak: %{size}", size = format_file_size(peak))
                ));
            }
            if let Some(private) = proc_mem.private_bytes {
                summary.push_str(&format!(
                    " · {}",
                    t!("Private: %{size}", size = format_file_size(private))
                ));
            }
            lines.push(summary);
        }
        lines.push(
            t!(
                "Cover cache: %{count} images (%{size}) · Decoded textures: %{tex}/%{tex_cap}",
                count = app.cover_source_count,
                size = format_file_size(app.cover_source_bytes),
                tex = app.cover_texture_count,
                tex_cap = app.cover_texture_capacity
            )
            .to_string(),
        );
        lines.push(
            t!(
                "Cover thumbnails: %{count} (%{size}) · Default & vinyl: %{extra}",
                count = app.cover_thumbnail_count,
                size = format_file_size(app.cover_thumbnail_bytes),
                extra = format_file_size(app.default_cover_bytes.saturating_add(app.vinyl_bytes))
            )
            .to_string(),
        );
        lines.push(
            t!(
                "Lyrics cache: %{count} entries (%{size}) · Log buffer: %{logs}",
                count = app.lyrics_bodies,
                size = format_file_size(app.lyrics_bytes),
                logs = format_file_size(app.log_buffer_bytes)
            )
            .to_string(),
        );
        lines.push(
            t!(
                "Library & lists: %{library} items · Queue: %{queue} · Playlist: %{playlist} (index %{index})",
                library = app.library_items,
                queue = app.queue_entries,
                playlist = app.playlist_songs,
                index = format_file_size(app.playlist_search_bytes)
            )
            .to_string(),
        );
        if let Some(other) = snap.untracked_rss_bytes() {
            lines.push(
                t!(
                    "Tracked caches: %{tracked} · GPU/UI & runtime overhead: %{other}",
                    tracked = format_file_size(app.tracked_bytes()),
                    other = format_file_size(other)
                )
                .to_string(),
            );
        }
        lines
    }

    fn log_playback_details(&self) -> Vec<String> {
        let state = &self.logs.playback;
        let status = if state.worker_closed {
            t!("Audio worker stopped")
        } else if let Some(stall) = state.stall {
            t!("Buffering %{seconds}s", seconds = stall.as_secs())
        } else if state.paused {
            t!("Paused")
        } else if state.active {
            t!("Playing")
        } else {
            t!("Stopped")
        };
        let mut details = vec![
            t!("Playback status: %{status}", status = status).to_string(),
            t!(
                "Position: %{position} · Buffered: %{buffered}",
                position = format_playback(state.position),
                buffered = format_playback(state.buffered)
            )
            .to_string(),
            t!(
                "Output: %{device} · %{rate} Hz · %{channels} channels",
                device = state
                    .output_name
                    .clone()
                    .unwrap_or_else(|| t!("System default").to_string()),
                rate = state.output_sample_rate,
                channels = state.output_channels
            )
            .to_string(),
        ];
        if let Some(error) = &state.error {
            details.push(t!("Playback error: %{error}", error = sanitize_urls(error)).to_string());
        }
        details
    }

    fn copy_log_report(&mut self, cx: &mut Context<Self>) {
        let time = self
            .logs
            .updated_at
            .map(|time| time.to_rfc3339())
            .unwrap_or_default();
        let playback = &self.logs.playback;
        let report = format!(
            "Navidrome Client {} ({}/{})\nSnapshot: {time}\n{}\nactive={} paused={} ended={} worker_closed={} track_attached={} fully_buffered={} stall_ms={:?} failure={:?}\n\nMemory:\n{}\n\n{}\n{}",
            env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH,
            self.log_playback_details().join("\n"), playback.active, playback.paused,
            playback.ended, playback.worker_closed, playback.output_connected,
            playback.fully_buffered, playback.stall.map(|stall| stall.as_millis()),
            playback.failure, self.log_memory_details().join("\n"),
            self.logs.filter.label(), self.logs.rendered
        );
        cx.write_to_clipboard(ClipboardItem::new_string(report));
        self.notify_success(t!("Log report copied."));
        cx.notify();
    }

    pub(in crate::app) fn render_log_settings(&self, cx: &Context<Self>) -> gpui_kit::AnyElement {
        let app = cx.entity().downgrade();
        let filter = self.logs.filter;
        let p = palette(cx);
        let dark = cx.theme().is_dark();

        let file_logging_card = settings_group(
            None,
            None,
            vec![
                settings_row(
                    t!("Write logs to file"),
                    Some(
                        t!("Save logs for later troubleshooting. Changes take effect immediately.")
                            .into(),
                    ),
                    Switch::new("file-logging-enabled")
                        .checked(self.config.log_to_file)
                        .accessibility_label(t!("Write logs to file"))
                        .on_change(
                            cx.listener(|this, checked, _, cx| this.set_file_logging(*checked, cx)),
                        )
                        .into_any_element(),
                    cx,
                ),
                h_flex()
                    .w_full()
                    .px_4()
                    .py_2p5()
                    .justify_between()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(p.text_muted)
                            .child(config::log_path().display().to_string()),
                    )
                    .child(
                        surface_button("open-log-directory")
                            .label(t!("Open log directory"))
                            .compact_action()
                            .on_click(cx.listener(|this, _, _, cx| {
                                match crate::logging::open_directory() {
                                    Ok(directory) => cx.open_with_system(&directory),
                                    Err(error) => this.notify_error(t!(
                                        "Could not open log directory: %{error}",
                                        error = error
                                    )),
                                }
                            })),
                    )
                    .into_any_element(),
            ],
            cx,
        );

        let snapshot_card = self.logs.updated_at.is_some().then(|| {
            settings_group(
                Some(t!("Playback snapshot").into()),
                None,
                vec![v_flex()
                    .w_full()
                    .px_4()
                    .py_3()
                    .gap_1()
                    .children(self.log_playback_details().into_iter().map(|detail| {
                        div()
                            .text_xs()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(p.text_secondary)
                            .child(detail)
                    }))
                    .into_any_element()],
                cx,
            )
        });

        let memory_card = self.logs.updated_at.is_some().then(|| {
            settings_group(
                Some(t!("Memory usage").into()),
                None,
                vec![
                    v_flex()
                        .w_full()
                        .px_4()
                        .py_3()
                        .gap_1()
                        .children(self.log_memory_details().into_iter().map(|detail| {
                            div()
                                .text_xs()
                                .font_family(cx.theme().mono_font_family.clone())
                                .text_color(p.text_secondary)
                                .child(detail)
                        }))
                        .into_any_element(),
                    h_flex()
                        .w_full()
                        .px_4()
                        .py_2p5()
                        .justify_between()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_xs()
                                .text_color(p.text_muted)
                                .child(t!(
                                    "Release cached cover images and decoded textures from memory. Covers reload from disk when scrolled back into view."
                                )),
                        )
                        .child(
                            surface_button("purge-cover-memory-cache")
                                .label(t!("Release image memory"))
                                .compact_action()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    let (count, freed) = this.purge_cover_memory_cache(window, cx);
                                    this.logs.memory = this.sample_memory_snapshot();
                                    this.notify_success(t!(
                                        "Released %{count} cached covers (%{size})",
                                        count = count,
                                        size = format_file_size(freed)
                                    ));
                                    cx.notify();
                                })),
                        )
                        .into_any_element(),
                ],
                cx,
            )
        });

        let viewer_card =
            settings_card(cx)
                .child(
                    h_flex()
                        .w_full()
                        .px_4()
                        .py_3()
                        .flex_wrap()
                        .justify_between()
                        .items_center()
                        .gap_2()
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    surface_button("log-filter")
                                        .label(filter.label())
                                        .dropdown_caret(true)
                                        .compact_action()
                                        .dropdown_menu(move |menu, _, cx| {
                                            app.update(cx, |_, cx| {
                                                [
                                                    LogFilter::All,
                                                    LogFilter::WarningsAndErrors,
                                                    LogFilter::Playback,
                                                ]
                                                .into_iter()
                                                .fold(menu, |menu, choice| {
                                                    menu.item(
                                                        PopupMenuItem::new(choice.label())
                                                            .checked(choice == filter)
                                                            .on_click(cx.listener(
                                                                move |this, _, _, cx| {
                                                                    this.logs.filter = choice;
                                                                    this.logs.dirty = true;
                                                                    cx.notify();
                                                                },
                                                            )),
                                                    )
                                                })
                                            })
                                            .expect("app owns log filter menu")
                                        }),
                                )
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_2()
                                        .pl_1()
                                        .child(
                                            Switch::new("auto-refresh-logs")
                                                .checked(self.logs.auto_refresh)
                                                .accessibility_label(t!("Auto refresh"))
                                                .on_change(cx.listener(|this, checked, _, cx| {
                                                    this.logs.auto_refresh = *checked;
                                                    if *checked {
                                                        this.logs.refresh();
                                                    }
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(p.text_secondary)
                                                .child(t!("Auto refresh")),
                                        ),
                                ),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    surface_button("refresh-logs")
                                        .icon(AppIcon::Refresh)
                                        .label(t!("Refresh logs"))
                                        .compact_action()
                                        .disabled(self.logs.pending.is_some())
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.logs.refresh();
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    surface_button("copy-logs")
                                        .label(t!("Copy log report"))
                                        .compact_action()
                                        .disabled(self.logs.updated_at.is_none() || self.logs.dirty)
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.copy_log_report(cx)),
                                        ),
                                ),
                        ),
                )
                .child(settings_divider(cx))
                .child(
                    v_flex()
                        .w_full()
                        .p_3()
                        .gap_2p5()
                        .child(Input::new(&self.logs.search).w_full().bg(p.sunken))
                        .child(
                            h_flex()
                                .flex_wrap()
                                .justify_between()
                                .px_0p5()
                                .gap_2()
                                .child(div().text_xs().text_color(p.text_muted).child(t!(
                                    "%{count} entries · Newest first",
                                    count = self.logs.matching
                                )))
                                .when_some(self.logs.updated_at, |this, time| {
                                    this.child(div().text_xs().text_color(p.text_muted).child(t!(
                                        "Updated %{time}",
                                        time = time.format("%H:%M:%S")
                                    )))
                                }),
                        )
                        .when_some(self.logs.error.as_ref(), |this, error| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().danger)
                                    .child(t!("Could not read logs: %{error}", error = error)),
                            )
                        })
                        .when_some(self.logs.file_error.as_ref(), |this, error| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().danger)
                                    .child(t!("Could not write log file: %{error}", error = error)),
                            )
                        })
                        .child(
                            div()
                                .w_full()
                                .rounded(px(tokens::radius::CONTROL))
                                .bg(p.sunken)
                                .border_1()
                                .border_color(if dark {
                                    hsla(0.0, 0.0, 1.0, 0.06)
                                } else {
                                    p.border.opacity(0.75)
                                })
                                .overflow_hidden()
                                .child(
                                    Textarea::new(&self.logs.text)
                                        .readonly(true)
                                        .w_full()
                                        .h(px(280.0))
                                        .text_xs()
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .aria_label(t!("Logs")),
                                ),
                        )
                        .when(
                            self.logs.updated_at.is_some()
                                && self.logs.matching == 0
                                && self.logs.error.is_none(),
                            |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(p.text_secondary)
                                        .child(t!("No matching log entries.")),
                                )
                            },
                        ),
                );

        v_flex()
            .w_full()
            .gap_4()
            .child(file_logging_card)
            .children(snapshot_card)
            .children(memory_card)
            .child(viewer_card)
            .child(div().px_1().text_xs().text_color(p.text_muted).child(t!(
                "Recent logs only (up to %{count} entries). Pause auto refresh to select text. File output can be disabled without stopping this viewer.",
                count = MAX_LOG_ENTRIES
            )))
            .into_any_element()
    }
}
