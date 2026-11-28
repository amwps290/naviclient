//! Inline whole-song lyric timing. Nudges and click-to-align take effect and
//! are saved at once, so the listener adjusts by ear while the song plays.

use super::lyrics_state::LyricsState;
use crate::app::prelude::*;
use crate::lyrics::{cache::LyricsCache, Document};
use crate::source::MediaKey;

/// Clicked lines are aligned on this grid; a tap is never more exact than that.
const ALIGN_STEP_MS: i64 = 10;

/// What the timing bar asks its host to do with the playing song.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimingAction {
    /// Shift the whole song by milliseconds; positive shows lyrics later.
    Nudge(i64),
    /// Arm aligning the next clicked line, or give that up.
    ToggleAlign,
    Reset,
    Close,
}

/// The inline timing bar belongs to one song and closes when another is shown.
#[derive(Default)]
pub(in crate::app) struct TimingBar {
    media: Option<MediaKey>,
    aligning: bool,
    display_line: Option<usize>,
}

impl TimingBar {
    pub(in crate::app) fn is_open(&self) -> bool {
        self.media.is_some()
    }

    /// The next click on a lyric line aligns it instead of seeking to it.
    pub(in crate::app) fn is_aligning(&self) -> bool {
        self.aligning
    }

    /// Keep the last rendered focus while a line is being picked. Focus also
    /// controls font size, padding and word layout, not just scroll position.
    pub(super) fn display_line(&self, current: Option<usize>) -> Option<usize> {
        if self.aligning {
            self.display_line
        } else {
            current
        }
    }

    fn open(&mut self, media: MediaKey) {
        self.media = Some(media);
        self.aligning = false;
    }

    fn close(&mut self) {
        *self = Self::default();
    }

    pub(in crate::app) fn set_aligning(&mut self, aligning: bool) {
        self.aligning = aligning && self.is_open();
    }

    /// Open the bar for the song on screen, or close it.
    fn toggle(&mut self, media: Option<&MediaKey>) {
        if self.is_open() {
            self.close();
        } else if let Some(media) = media {
            self.open(media.clone());
        }
    }

    /// Carry out what the bar asked for on the lyrics of the playing song.
    fn perform(
        &mut self,
        action: TimingAction,
        lyrics: &mut LyricsState,
        cache: &LyricsCache,
    ) -> anyhow::Result<()> {
        match action {
            TimingAction::Nudge(delta_ms) => lyrics.change_timing(cache, |document| {
                document.offset_ms = document.offset_ms.saturating_add(delta_ms)
            })?,
            TimingAction::ToggleAlign => self.set_aligning(!self.aligning),
            TimingAction::Reset => lyrics.change_timing(cache, reset)?,
            TimingAction::Close => self.close(),
        }
        Ok(())
    }

    /// Whether a pending alignment took this click on a lyric line; otherwise
    /// the click is left to seek. One click is one attempt, saved or not.
    fn align_click(
        &mut self,
        line: usize,
        position: Duration,
        lyrics: &mut LyricsState,
        cache: &LyricsCache,
    ) -> anyhow::Result<bool> {
        if !std::mem::take(&mut self.aligning) {
            return Ok(false);
        }
        lyrics.change_timing(cache, |document| align(document, line, position))?;
        Ok(true)
    }

    /// The bar lasts while its song shows synced lyrics. A pending alignment
    /// also needs those lyrics on screen to be clicked.
    fn sync(
        &mut self,
        media: Option<&MediaKey>,
        synced: bool,
        visible: bool,
        display_line: Option<usize>,
    ) {
        if !synced || self.media.as_ref() != media {
            self.close();
            return;
        }
        if !visible {
            self.aligning = false;
        }
        if !self.aligning {
            self.display_line = display_line;
        }
    }
}

/// Escape must run before a focused menu or dialog gets its Cancel action.
/// A host can defer to shortcut recording by returning no timing bar.
fn timing_escape<V: 'static>(
    cx: &mut Context<V>,
    bar: fn(&mut V) -> Option<&mut TimingBar>,
) -> Subscription {
    let host = cx.entity().downgrade();
    cx.intercept_keystrokes(move |event, _, cx| {
        if event.keystroke.key != "escape" {
            return;
        }
        let _ = host.update(cx, |this, cx| {
            if let Some(bar) = bar(this).filter(|bar| bar.is_aligning()) {
                bar.set_aligning(false);
                cx.stop_propagation();
                cx.notify();
            }
        });
    })
}

/// Make `line` start at `position`; an untimed line changes nothing. The
/// offset comes from the line's own timestamp, because the displayed one is
/// clamped at zero.
fn align(document: &mut Document, line: usize, position: Duration) {
    let Some(start_ms) = document
        .edited_lyrics()
        .lines
        .get(line)
        .and_then(|line| line.start_ms)
    else {
        return;
    };
    let position = i64::try_from(position.as_millis()).unwrap_or(i64::MAX);
    let offset = position.saturating_sub(i64::try_from(start_ms).unwrap_or(i64::MAX));
    // Nearest step, with halves rounded away from zero.
    document.offset_ms = offset.saturating_add(offset.signum() * (ALIGN_STEP_MS / 2))
        / ALIGN_STEP_MS
        * ALIGN_STEP_MS;
}

fn reset(document: &mut Document) {
    document.offset_ms = 0;
    document.timing_edits = None;
}

fn adjusted(document: &Document) -> bool {
    document.offset_ms != 0 || document.timing_edits.is_some()
}

/// How far the lyrics are shifted, in words rather than as a signed number.
pub(super) fn shift_label(offset_ms: i64) -> SharedString {
    let ms = offset_ms.unsigned_abs();
    // Exactly as many decimals as the offset has, and at least one.
    let seconds = format!("{}.{:03}", ms / 1000, ms % 1000);
    let seconds = seconds.trim_end_matches('0');
    let seconds = if seconds.ends_with('.') {
        format!("{seconds}0")
    } else {
        seconds.to_string()
    };
    match offset_ms.signum() {
        0 => t!("In sync"),
        1 => t!("%{seconds} s later", seconds = seconds),
        _ => t!("%{seconds} s earlier", seconds = seconds),
    }
    .into()
}

/// The bar for `document`. Every control reports to `host`, so the same bar
/// runs in the player and on plain state in tests.
fn timing_bar<V: 'static>(
    document: &Document,
    aligning: bool,
    cx: &Context<V>,
    host: fn(&mut V, TimingAction, &mut Context<V>),
) -> gpui_kit::AnyElement {
    let p = palette(cx);
    let dark = cx.theme().is_dark();
    let shift = shift_label(document.offset_ms);
    let is_adjusted = adjusted(document);
    let hint: SharedString = if aligning {
        t!("Click a line the moment it starts being sung. Press Esc to cancel.")
    } else {
        t!("Timing changes apply to this song only.")
    }
    .into();
    let card_bg = if dark {
        hsla(0.0, 0.0, 1.0, 0.045)
    } else {
        p.raised.opacity(0.76)
    };
    let card_border = if aligning {
        p.brand.opacity(if dark { 0.42 } else { 0.34 })
    } else if dark {
        hsla(0.0, 0.0, 1.0, 0.09)
    } else {
        p.border.opacity(0.82)
    };
    let stepper_bg = if dark {
        hsla(0.0, 0.0, 0.0, 0.24)
    } else {
        p.sunken.opacity(0.72)
    };
    let stepper_border = if dark {
        hsla(0.0, 0.0, 1.0, 0.07)
    } else {
        p.border.opacity(0.75)
    };
    let divider = || {
        div()
            .w(px(1.0))
            .h(px(14.0))
            .flex_none()
            .bg(if dark {
                hsla(0.0, 0.0, 1.0, 0.08)
            } else {
                p.border.opacity(0.85)
            })
    };
    let step = |id: &'static str, label: &'static str, delta_ms: i64| {
        ghost_button(id)
            .debug_selector(move || id.into())
            .label(label)
            .tooltip(shift_label(delta_ms))
            .rounded_full()
            .h(px(26.0))
            .px_2()
            .text_xs()
            .font_family(cx.theme().mono_font_family.clone())
            .font_weight(FontWeight::MEDIUM)
            .on_click(
                cx.listener(move |this, _, _, cx| host(this, TimingAction::Nudge(delta_ms), cx)),
            )
    };
    let steps = h_flex()
        .min_w_0()
        .flex_none()
        .px_1()
        .py_0p5()
        .items_center()
        .gap_0p5()
        .rounded_full()
        .bg(stepper_bg)
        .border_1()
        .border_color(stepper_border)
        .child(step("lyrics-earlier-500", "−0.5", -500))
        .child(step("lyrics-earlier-100", "−0.1", -100))
        .child(divider())
        .child(
            // A fixed slot keeps the buttons still while the amount changes.
            div()
                .id("lyrics-timing-shift")
                .w(px(tokens::LYRIC_TIMING_STATUS_WIDTH))
                .flex_none()
                .px_1()
                .text_center()
                .text_xs()
                .font_weight(if document.offset_ms == 0 {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::SEMIBOLD
                })
                .text_ellipsis()
                .text_color(if document.offset_ms == 0 {
                    p.text_secondary
                } else {
                    p.brand
                })
                .child(shift.clone())
                .tooltip(move |window, cx| Tooltip::new(shift.clone()).build(window, cx)),
        )
        .child(divider())
        .child(step("lyrics-later-100", "+0.1", 100))
        .child(step("lyrics-later-500", "+0.5", 500));
    let actions = h_flex()
        .items_center()
        .gap_1()
        .child(
            Button::new("lyrics-align")
                .debug_selector(|| "lyrics-align".into())
                .label(t!("Align to clicked line"))
                .custom(soft_toggle_style(aligning, cx))
                .selected(aligning)
                .toggled(aligning)
                .rounded_full()
                .h(px(28.0))
                .px_3()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .cursor_pointer()
                .when(aligning, |this| {
                    this.border_1()
                        .border_color(p.brand.opacity(if dark { 0.36 } else { 0.28 }))
                })
                .on_click(
                    cx.listener(move |this, _, _, cx| host(this, TimingAction::ToggleAlign, cx)),
                ),
        )
        .child(
            ghost_button("lyrics-timing-reset")
                .debug_selector(|| "lyrics-timing-reset".into())
                .label(t!("Reset"))
                .rounded_full()
                .h(px(28.0))
                .px_2p5()
                .text_xs()
                .disabled(!is_adjusted)
                .on_click(cx.listener(move |this, _, _, cx| host(this, TimingAction::Reset, cx))),
        )
        .child(
            icon_button(
                "lyrics-timing-close",
                AppIcon::Close,
                t!("Close"),
                tokens::ICON_BUTTON_XS,
            )
            .debug_selector(|| "lyrics-timing-close".into())
            .on_click(cx.listener(move |this, _, _, cx| host(this, TimingAction::Close, cx))),
        );
    v_flex()
        .w_full()
        .mt_1()
        .px_3()
        .py_2()
        .gap_1p5()
        .rounded(px(tokens::radius::COVER))
        .bg(card_bg)
        .border_1()
        .border_color(card_border)
        .child(
            h_flex()
                .w_full()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap_x_2p5()
                .gap_y_1p5()
                .child(steps)
                .child(actions),
        )
        .child(
            // Always one line, so arming an alignment moves no lyric.
            h_flex()
                .w_full()
                .h(px(tokens::LYRIC_TIMING_HINT_HEIGHT))
                .flex_none()
                .items_center()
                .gap_1p5()
                .child(
                    div()
                        .size(px(5.0))
                        .flex_none()
                        .rounded_full()
                        .bg(if aligning {
                            p.brand
                        } else {
                            p.text_muted.opacity(0.55)
                        }),
                )
                .child(
                    div()
                        .id("lyrics-timing-hint")
                        .flex_1()
                        .min_w_0()
                        .h(px(tokens::LYRIC_TIMING_HINT_HEIGHT))
                        .text_xs()
                        .line_height(px(tokens::LYRIC_TIMING_HINT_HEIGHT))
                        .text_ellipsis()
                        .text_color(if aligning { p.brand } else { p.text_muted })
                        .child(hint.clone())
                        .tooltip(move |window, cx| Tooltip::new(hint.clone()).build(window, cx)),
                ),
        )
        .into_any_element()
}

impl NavidromeApp {
    pub(in crate::app) fn subscribe_lyrics_timing_keys(&mut self, cx: &mut Context<Self>) {
        self._subscriptions.push(timing_escape(cx, |this| {
            if this.state.settings.recording().is_some() {
                None
            } else {
                Some(&mut this.lyrics_ui.timing)
            }
        }));
    }

    /// Close the bar once its song or its synced lyrics are no longer shown.
    pub(super) fn sync_lyrics_timing(&mut self) {
        let lyrics = self.state.lyrics.lyrics();
        let synced = lyrics.is_some_and(|lyrics| lyrics.is_synced());
        let display_line = lyrics
            .and_then(|lyrics| lyrics.display_line_index(self.playback_for_display().position));
        self.lyrics_ui.timing.sync(
            self.state.lyrics.media(),
            synced,
            self.state.view == View::NowPlaying,
            display_line,
        );
    }

    pub(super) fn toggle_lyrics_timing(&mut self, cx: &mut Context<Self>) {
        self.lyrics_ui.timing.toggle(self.state.lyrics.media());
        cx.notify();
    }

    fn lyrics_timing_action(&mut self, action: TimingAction, cx: &mut Context<Self>) {
        let done =
            self.lyrics_ui
                .timing
                .perform(action, &mut self.state.lyrics, &self.lyrics_cache);
        if let Err(error) = done {
            self.notify_error(t!("Could not save lyrics: %{error}", error = error));
        }
        cx.notify();
    }

    /// A click on a lyric line seeks to it, unless the timing bar is waiting
    /// for the line that is being sung.
    pub(super) fn click_lyric_line(&mut self, line: usize, start_ms: u64, cx: &mut Context<Self>) {
        let position = self.playback_for_display().position;
        let aligned = self.lyrics_ui.timing.align_click(
            line,
            position,
            &mut self.state.lyrics,
            &self.lyrics_cache,
        );
        match aligned {
            Ok(true) => {}
            Ok(false) => self.seek_playback(Duration::from_millis(start_ms)),
            Err(error) => self.notify_error(t!("Could not save lyrics: %{error}", error = error)),
        }
        cx.notify();
    }

    pub(super) fn render_lyrics_timing_bar(
        &self,
        cx: &Context<Self>,
    ) -> Option<gpui_kit::AnyElement> {
        let timing = &self.lyrics_ui.timing;
        let document = self.state.lyrics.document.as_ref()?;
        timing.is_open().then(|| {
            timing_bar(
                document,
                timing.is_aligning(),
                cx,
                Self::lyrics_timing_action,
            )
        })
    }
}

#[cfg(test)]
mod tests;
