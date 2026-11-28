//! Standalone borderless, transparent, always-on-top Desktop Overlay Lyrics window.

use gpui_kit::{
    div, hsla, point, px, size, AnyWindowHandle, Context, FocusHandle, InteractiveElement,
    IntoElement, MouseButton, ParentElement, Render, StatefulInteractiveElement, Styled,
    WeakEntity, Window, WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowKind,
    WindowOptions,
};

use super::word_lyrics::WordLyrics;
use crate::app::prelude::*;
use crate::models::{Lyrics, WordTimedLine};

const OVERLAY_DEFAULT_WIDTH: f32 = 760.0;
const OVERLAY_TWO_LINE_HEIGHT: f32 = 136.0;
const OVERLAY_ONE_LINE_HEIGHT: f32 = 96.0;
pub(in crate::app) const OVERLAY_MIN_FONT: u16 = 16;
pub(in crate::app) const OVERLAY_MAX_FONT: u16 = 44;

#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::app) struct OverlayLyricSnapshot {
    pub(in crate::app) primary_text: String,
    pub(in crate::app) primary_words: Option<WordTimedLine>,
    pub(in crate::app) secondary_text: Option<String>,
    pub(in crate::app) position_ms: u64,
    pub(in crate::app) song_title: String,
    pub(in crate::app) song_artist: String,
    pub(in crate::app) paused: bool,
    pub(in crate::app) active: bool,
    pub(in crate::app) word_timing: bool,
    pub(in crate::app) two_line: bool,
    pub(in crate::app) font_size: u16,
    pub(in crate::app) locked: bool,
}

#[derive(Clone, Copy, Debug)]
pub(in crate::app) struct OverlaySnapshotOptions {
    pub(in crate::app) position: Duration,
    pub(in crate::app) paused: bool,
    pub(in crate::app) active: bool,
    pub(in crate::app) word_timing: bool,
    pub(in crate::app) two_line: bool,
    pub(in crate::app) show_translation: bool,
    pub(in crate::app) font_size: u16,
    pub(in crate::app) locked: bool,
}

/// Pure projection of current lyrics + playback position into 1 or 2 overlay lines.
pub(in crate::app) fn build_overlay_snapshot(
    lyrics: Option<&Lyrics>,
    song: Option<&Song>,
    opts: OverlaySnapshotOptions,
) -> OverlayLyricSnapshot {
    let song_title = song.map(|s| s.title.clone()).unwrap_or_default();
    let song_artist = song.map(|s| s.artist.clone()).unwrap_or_default();
    let position_ms = opts.position.as_millis().min(u64::MAX as u128) as u64;

    let mut snap = OverlayLyricSnapshot {
        primary_text: String::new(),
        primary_words: None,
        secondary_text: None,
        position_ms,
        song_title: song_title.clone(),
        song_artist: song_artist.clone(),
        paused: opts.paused,
        active: opts.active,
        word_timing: opts.word_timing,
        two_line: opts.two_line,
        font_size: opts.font_size.clamp(OVERLAY_MIN_FONT, OVERLAY_MAX_FONT),
        locked: opts.locked,
    };

    if let Some(lyrics) = lyrics.filter(|l| !l.lines.is_empty()) {
        if let Some(idx) = lyrics.display_line_index(opts.position) {
            if let Some(line) = lyrics.lines.get(idx) {
                snap.primary_text = line.text.trim().to_string();
                if opts.word_timing {
                    snap.primary_words = line.word_timing.clone();
                }
                if opts.two_line {
                    let translation = opts
                        .show_translation
                        .then(|| {
                            line.translation
                                .as_deref()
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(str::to_string)
                        })
                        .flatten();
                    snap.secondary_text = translation.or_else(|| {
                        lyrics
                            .lines
                            .get(idx + 1)
                            .map(|next| next.text.trim().to_string())
                            .filter(|s| !s.is_empty())
                    });
                }
                return snap;
            }
        }
        // Before the first synced timestamp or plain lyrics: show first non-empty line.
        if let Some(first) = lyrics.lines.iter().find(|l| !l.text.trim().is_empty()) {
            snap.primary_text = first.text.trim().to_string();
            return snap;
        }
    }

    if !song_title.is_empty() {
        snap.primary_text = if song_artist.is_empty() {
            song_title
        } else {
            format!("{song_title} — {song_artist}")
        };
    } else {
        snap.primary_text = t!("No track playing").to_string();
    }
    snap
}

#[derive(Default)]
pub(in crate::app) struct OverlayLyricsState {
    window: Option<AnyWindowHandle>,
    view: Option<WeakEntity<DesktopLyricsView>>,
}

impl OverlayLyricsState {
    pub(in crate::app) fn is_open(&self) -> bool {
        self.window.is_some()
    }
}

pub(in crate::app) struct DesktopLyricsView {
    app: WeakEntity<NavidromeApp>,
    snapshot: OverlayLyricSnapshot,
    hovered: bool,
    focus: FocusHandle,
}

impl DesktopLyricsView {
    fn new(
        app: WeakEntity<NavidromeApp>,
        snapshot: OverlayLyricSnapshot,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            app,
            snapshot,
            hovered: false,
            focus: cx.focus_handle(),
        }
    }

    fn update_snapshot(
        &mut self,
        snapshot: OverlayLyricSnapshot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lock_changed = self.snapshot.locked != snapshot.locked;
        let line_mode_changed = self.snapshot.two_line != snapshot.two_line;
        if self.snapshot == snapshot {
            return;
        }
        self.snapshot = snapshot;
        if lock_changed {
            set_click_through(window, self.snapshot.locked);
        }
        if line_mode_changed {
            let target_h = if self.snapshot.two_line {
                OVERLAY_TWO_LINE_HEIGHT
            } else {
                OVERLAY_ONE_LINE_HEIGHT
            };
            let current_w = f32::from(window.viewport_size().width).max(460.0);
            window.resize(size(px(current_w), px(target_h)));
        }
        cx.notify();
    }
}

impl Render for DesktopLyricsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snap = &self.snapshot;
        let locked = snap.locked;
        let show_chrome = self.hovered && !locked;
        let font_px = snap.font_size as f32;
        let secondary_px = (font_px * 0.62).clamp(12.0, 22.0);
        let accent = palette(cx).brand;
        let base_color = hsla(0.0, 0.0, 0.96, 0.95);
        let dim_color = hsla(0.0, 0.0, 0.90, 0.72);

        let primary_el: gpui_kit::AnyElement = if let Some(words) = snap.primary_words.clone() {
            div()
                .w_full()
                .flex()
                .justify_center()
                .text_size(px(font_px))
                .font_weight(FontWeight::BOLD)
                .line_height(px(font_px * 1.32))
                .child(WordLyrics::new(words, snap.position_ms, dim_color, accent))
                .into_any_element()
        } else {
            div()
                .w_full()
                .text_center()
                .text_size(px(font_px))
                .font_weight(FontWeight::BOLD)
                .line_height(px(font_px * 1.32))
                .text_color(base_color)
                .truncate()
                .child(snap.primary_text.clone())
                .into_any_element()
        };

        let app_prev = self.app.clone();
        let app_toggle = self.app.clone();
        let app_next = self.app.clone();
        let app_smaller = self.app.clone();
        let app_larger = self.app.clone();
        let app_lines = self.app.clone();
        let app_lock = self.app.clone();
        let app_close = self.app.clone();

        let toolbar = h_flex()
            .id("overlay-lyrics-toolbar")
            .h(px(30.0))
            .px_2p5()
            .gap_1()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(hsla(0.0, 0.0, 0.08, 0.78))
            .border_1()
            .border_color(hsla(0.0, 0.0, 1.0, 0.14))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                icon_button(
                    "overlay-prev",
                    PlayerIcon::Previous,
                    t!("Previous"),
                    tokens::ICON_BUTTON_XS,
                )
                .on_click(move |_, _, cx| {
                    let _ = app_prev.update(cx, |this, cx| {
                        this.advance_queue(false);
                        cx.notify();
                    });
                }),
            )
            .child(
                icon_button(
                    "overlay-play-pause",
                    if snap.paused || !snap.active {
                        PlayerIcon::Play
                    } else {
                        PlayerIcon::Pause
                    },
                    if snap.paused || !snap.active {
                        t!("Play")
                    } else {
                        t!("Pause")
                    },
                    tokens::ICON_BUTTON_XS,
                )
                .on_click(move |_, _, cx| {
                    let _ = app_toggle.update(cx, |this, cx| {
                        this.toggle_playback();
                        cx.notify();
                    });
                }),
            )
            .child(
                icon_button(
                    "overlay-next",
                    PlayerIcon::Next,
                    t!("Next"),
                    tokens::ICON_BUTTON_XS,
                )
                .on_click(move |_, _, cx| {
                    let _ = app_next.update(cx, |this, cx| {
                        this.advance_queue(true);
                        cx.notify();
                    });
                }),
            )
            .child(div().w(px(1.0)).h(px(14.0)).mx_1().bg(hsla(0.0, 0.0, 1.0, 0.16)))
            .child(
                ghost_button("overlay-font-smaller")
                    .label("A-")
                    .compact_action()
                    .disabled(snap.font_size <= OVERLAY_MIN_FONT)
                    .on_click(move |_, _, cx| {
                        let _ = app_smaller.update(cx, |this, cx| {
                            this.adjust_overlay_lyrics_font(-2, cx);
                        });
                    }),
            )
            .child(
                ghost_button("overlay-font-larger")
                    .label("A+")
                    .compact_action()
                    .disabled(snap.font_size >= OVERLAY_MAX_FONT)
                    .on_click(move |_, _, cx| {
                        let _ = app_larger.update(cx, |this, cx| {
                            this.adjust_overlay_lyrics_font(2, cx);
                        });
                    }),
            )
            .child(
                ghost_button("overlay-line-mode")
                    .label(if snap.two_line {
                        t!("2 lines")
                    } else {
                        t!("1 line")
                    })
                    .compact_action()
                    .on_click(move |_, _, cx| {
                        let _ = app_lines.update(cx, |this, cx| {
                            this.toggle_overlay_lyrics_two_line(cx);
                        });
                    }),
            )
            .child(
                ghost_button("overlay-lock")
                    .label(t!("Lock"))
                    .compact_action()
                    .on_click(move |_, _, cx| {
                        let _ = app_lock.update(cx, |this, cx| {
                            this.set_overlay_lyrics_locked(true, cx);
                        });
                    }),
            )
            .child(
                icon_button(
                    "overlay-close",
                    AppIcon::Close,
                    t!("Close desktop lyrics"),
                    tokens::ICON_BUTTON_XS,
                )
                .on_click(move |_, _, cx| {
                    let _ = app_close.update(cx, |this, cx| {
                        this.close_overlay_lyrics(cx);
                    });
                }),
            );

        v_flex()
            .id("desktop-lyrics-root")
            .track_focus(&self.focus)
            .size_full()
            .px_5()
            .py_2p5()
            .justify_center()
            .items_center()
            .gap_1p5()
            .rounded(px(tokens::radius::LARGE))
            .bg(if show_chrome {
                hsla(0.0, 0.0, 0.06, 0.68)
            } else {
                hsla(0.0, 0.0, 0.04, 0.34)
            })
            .border_1()
            .border_color(if show_chrome {
                hsla(0.0, 0.0, 1.0, 0.16)
            } else {
                hsla(0.0, 0.0, 1.0, 0.04)
            })
            .shadow(vec![box_shadow(
                px(0.0),
                px(6.0),
                px(20.0),
                px(0.0),
                hsla(0.0, 0.0, 0.0, if show_chrome { 0.42 } else { 0.22 }),
            )])
            .on_hover(cx.listener(|this, hovering, _, cx| {
                this.hovered = *hovering;
                cx.notify();
            }))
            .on_mouse_down(MouseButton::Left, move |_, window, _| {
                if !locked {
                    window.start_window_move();
                }
            })
            .when(show_chrome, |this| this.child(toolbar))
            .child(primary_el)
            .when_some(
                snap.two_line
                    .then_some(snap.secondary_text.as_ref())
                    .flatten(),
                |this, secondary| {
                    this.child(
                        div()
                            .w_full()
                            .text_center()
                            .text_size(px(secondary_px))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(dim_color)
                            .truncate()
                            .child(secondary.clone()),
                    )
                },
            )
    }
}

impl NavidromeApp {
    pub(in crate::app) fn overlay_lyrics_open(&self) -> bool {
        self.overlay_lyrics.is_open()
    }

    pub(in crate::app) fn toggle_overlay_lyrics(&mut self, cx: &mut Context<Self>) {
        if self.overlay_lyrics.is_open() {
            self.close_overlay_lyrics(cx);
        } else {
            self.open_overlay_lyrics(cx);
        }
    }

    pub(in crate::app) fn open_overlay_lyrics(&mut self, cx: &mut Context<Self>) {
        if self.overlay_lyrics.is_open() {
            return;
        }
        let snapshot = self.current_overlay_snapshot();
        let height = if snapshot.two_line {
            OVERLAY_TWO_LINE_HEIGHT
        } else {
            OVERLAY_ONE_LINE_HEIGHT
        };
        let bounds = WindowBounds::Windowed(gpui_kit::Bounds {
            origin: point(px(220.0), px(640.0)),
            size: size(px(OVERLAY_DEFAULT_WIDTH), px(height)),
        });
        let options = WindowOptions {
            window_bounds: Some(bounds),
            titlebar: None,
            focus: false,
            show: true,
            kind: WindowKind::PopUp,
            is_movable: true,
            is_resizable: true,
            is_minimizable: false,
            window_background: WindowBackgroundAppearance::Transparent,
            window_decorations: Some(WindowDecorations::Client),
            ..Default::default()
        };
        let app_weak = cx.entity().downgrade();
        let locked = snapshot.locked;
        let Ok(handle) = cx.open_window(options, move |window, cx| {
            window.set_window_title("Desktop Lyrics");
            window.set_background_appearance(WindowBackgroundAppearance::Transparent);
            set_always_on_top(window, true);
            if locked {
                set_click_through(window, true);
            }
            cx.new(|cx| DesktopLyricsView::new(app_weak, snapshot, cx))
        }) else {
            self.notify_error(t!("Could not open desktop lyrics window"));
            return;
        };
        if let Ok(view_entity) = handle.entity(cx) {
            self.overlay_lyrics.view = Some(view_entity.downgrade());
        }
        self.overlay_lyrics.window = Some(handle.into());
        cx.notify();
    }

    pub(in crate::app) fn close_overlay_lyrics(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.overlay_lyrics.window.take() {
            let _ = handle.update(cx, |_, window, _| {
                window.remove_window();
            });
        }
        self.overlay_lyrics.view = None;
        cx.notify();
    }

    pub(in crate::app) fn sync_overlay_lyrics(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.overlay_lyrics.window else {
            return;
        };
        let snapshot = self.current_overlay_snapshot();
        let view = self.overlay_lyrics.view.clone();
        let updated = handle.update(cx, |_, window, cx| {
            if let Some(view) = view.as_ref().and_then(WeakEntity::upgrade) {
                view.update(cx, |view, cx| {
                    view.update_snapshot(snapshot, window, cx);
                });
            }
        });
        if updated.is_err() {
            self.overlay_lyrics.window = None;
            self.overlay_lyrics.view = None;
        }
    }

    pub(in crate::app) fn set_overlay_lyrics_locked(&mut self, locked: bool, cx: &mut Context<Self>) {
        let mut next = self.config.clone();
        next.lyrics.overlay.locked = locked;
        if let Err(error) = config::save(&next) {
            self.notify_error(t!("Could not save lyrics: %{error}", error = error));
            return;
        }
        self.config = next;
        self.sync_overlay_lyrics(cx);
        cx.notify();
    }

    pub(in crate::app) fn toggle_overlay_lyrics_two_line(&mut self, cx: &mut Context<Self>) {
        let mut next = self.config.clone();
        next.lyrics.overlay.two_line = !next.lyrics.overlay.two_line;
        if let Err(error) = config::save(&next) {
            self.notify_error(t!("Could not save lyrics: %{error}", error = error));
            return;
        }
        self.config = next;
        self.sync_overlay_lyrics(cx);
        cx.notify();
    }

    pub(in crate::app) fn adjust_overlay_lyrics_font(&mut self, delta: i16, cx: &mut Context<Self>) {
        let mut next = self.config.clone();
        let current = i32::from(next.lyrics.overlay.font_size);
        let updated = (current + i32::from(delta))
            .clamp(i32::from(OVERLAY_MIN_FONT), i32::from(OVERLAY_MAX_FONT)) as u16;
        if updated == next.lyrics.overlay.font_size {
            return;
        }
        next.lyrics.overlay.font_size = updated;
        if let Err(error) = config::save(&next) {
            self.notify_error(t!("Could not save lyrics: %{error}", error = error));
            return;
        }
        self.config = next;
        self.sync_overlay_lyrics(cx);
        cx.notify();
    }

    pub(in crate::app) fn current_overlay_snapshot(&self) -> OverlayLyricSnapshot {
        let playback = self.playback_for_display();
        let overlay_cfg = &self.config.lyrics.overlay;
        build_overlay_snapshot(
            self.state.lyrics.lyrics(),
            self.state.now_playing.as_ref(),
            OverlaySnapshotOptions {
                position: playback.position,
                paused: playback.paused,
                active: playback.active,
                word_timing: self.config.lyrics.word_timing,
                two_line: overlay_cfg.two_line,
                show_translation: overlay_cfg.show_translation
                    && self.config.show_lyrics_translation,
                font_size: overlay_cfg.font_size,
                locked: overlay_cfg.locked,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{LyricLine, Lyrics};

    #[test]
    fn overlay_snapshot_projects_synced_lines_translation_and_fallback() {
        let lyrics = Lyrics {
            lines: vec![
                LyricLine {
                    start_ms: Some(1_000),
                    text: "First line".into(),
                    translation: Some("第一行".into()),
                    ..Default::default()
                },
                LyricLine {
                    start_ms: Some(5_000),
                    text: "Second line".into(),
                    translation: None,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let song = Song {
            title: "Hello".into(),
            artist: "World".into(),
            ..Default::default()
        };

        let base = OverlaySnapshotOptions {
            position: Duration::from_millis(2_000),
            paused: false,
            active: true,
            word_timing: true,
            two_line: true,
            show_translation: true,
            font_size: 24,
            locked: false,
        };

        // At 2s with two_line + translation -> primary "First line", secondary "第一行"
        let snap = build_overlay_snapshot(Some(&lyrics), Some(&song), base);
        assert_eq!(snap.primary_text, "First line");
        assert_eq!(snap.secondary_text.as_deref(), Some("第一行"));

        // Translation off -> falls back to next line "Second line"
        let snap_next = build_overlay_snapshot(
            Some(&lyrics),
            Some(&song),
            OverlaySnapshotOptions {
                show_translation: false,
                ..base
            },
        );
        assert_eq!(snap_next.secondary_text.as_deref(), Some("Second line"));

        // One-line mode omits secondary line
        let snap_one = build_overlay_snapshot(
            Some(&lyrics),
            Some(&song),
            OverlaySnapshotOptions {
                two_line: false,
                font_size: 99,
                locked: true,
                ..base
            },
        );
        assert_eq!(snap_one.secondary_text, None);
        assert_eq!(snap_one.font_size, OVERLAY_MAX_FONT);
        assert!(snap_one.locked);

        // No lyrics -> falls back to song title & artist
        let snap_empty = build_overlay_snapshot(
            None,
            Some(&song),
            OverlaySnapshotOptions {
                position: Duration::ZERO,
                paused: true,
                active: false,
                word_timing: false,
                font_size: 10,
                ..base
            },
        );
        assert_eq!(snap_empty.primary_text, "Hello — World");
        assert_eq!(snap_empty.font_size, OVERLAY_MIN_FONT);
    }
}
