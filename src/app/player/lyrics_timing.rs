//! Whole-song vertical timing adjustment with a live lyric preview, isolated until saved.

use crate::app::prelude::*;
use crate::lyrics::Document;
use gpui_kit::component::dialog::Dialog;

const DRAG_MS_PER_PIXEL: f64 = 10.0;

struct TimingDrag {
    y: Pixels,
    scroll_y: Pixels,
    before: Document,
    moved: bool,
    following: bool,
}

pub(super) struct TimingEditor {
    pub draft: Document,
    pub fingerprint: String,
    pub scroll: ScrollHandle,
    pub anchor: Option<usize>,
    pub center_pending: bool,
    following: bool,
    precision_open: bool,
    undo: Vec<Document>,
    drag: Option<TimingDrag>,
}

impl TimingEditor {
    pub fn new(document: Document, position: Duration) -> Self {
        let lyrics = document.display_lyrics();
        let anchor = lyrics
            .display_line_index(position)
            .or_else(|| lyrics.lines.iter().position(|line| line.start_ms.is_some()));
        Self {
            fingerprint: document.timing_fingerprint(),
            draft: document,
            scroll: ScrollHandle::new(),
            anchor,
            center_pending: true,
            following: true,
            precision_open: false,
            undo: Vec::new(),
            drag: None,
        }
    }

    fn changed(a: &Document, b: &Document) -> bool {
        a.offset_ms != b.offset_ms || a.timing_edits != b.timing_edits
    }

    fn checkpoint(&mut self, before: Document) {
        if Self::changed(&before, &self.draft) {
            if self.undo.len() >= 32 {
                self.undo.remove(0);
            }
            self.undo.push(before);
        }
    }

    pub fn change(&mut self, change: impl FnOnce(&mut Document)) {
        self.cancel_drag();
        let before = self.draft.clone();
        change(&mut self.draft);
        self.checkpoint(before);
    }

    fn begin_drag(&mut self, y: Pixels) {
        self.cancel_drag();
        self.center_pending = false;
        self.drag = Some(TimingDrag {
            y,
            scroll_y: self.scroll.offset().y,
            before: self.draft.clone(),
            moved: false,
            following: self.following,
        });
        self.following = false;
    }

    fn update_drag(&mut self, y: Pixels) {
        let Some(drag) = &mut self.drag else {
            return;
        };
        let delta = f32::from(y - drag.y);
        if !drag.moved && delta.abs() < 4.0 {
            return;
        }
        drag.moved = true;
        let mut scroll_y = drag.scroll_y + px(delta);
        if self.scroll.bounds().size.height > px(0.0) {
            scroll_y = scroll_y.clamp(-self.scroll.max_offset().y, px(0.0));
        }
        let adjustment =
            (f64::from(f32::from(scroll_y - drag.scroll_y)) * DRAG_MS_PER_PIXEL).round() as i64;
        // Only pointer displacement changes the offset. Playback advancing and
        // large gaps between vocal lines must never make a held drag jump.
        self.draft.offset_ms = drag.before.offset_ms.saturating_add(adjustment);
        self.scroll.set_offset(point(px(0.0), scroll_y));
    }

    fn finish_drag(&mut self, y: Pixels, position: Duration) {
        self.update_drag(y);
        if let Some(drag) = self.drag.take() {
            self.following = drag.following;
            if drag.moved {
                self.checkpoint(drag.before);
                // Keep the released position; resume centering at the next cue.
                self.anchor = self.preview_anchor(position);
            }
        }
    }

    pub fn cancel_drag(&mut self) {
        if let Some(drag) = self.drag.take() {
            self.draft = drag.before;
            self.scroll.set_offset(point(px(0.0), drag.scroll_y));
            self.following = drag.following;
        }
    }

    fn preview_anchor(&self, position: Duration) -> Option<usize> {
        self.draft
            .display_lyrics()
            .display_line_index(position)
            .or_else(|| {
                self.draft
                    .lyrics
                    .lines
                    .iter()
                    .position(|line| line.start_ms.is_some())
            })
    }

    pub fn sync_playback(&mut self, position: Duration) {
        if self.following && self.drag.is_none() {
            let anchor = self.preview_anchor(position);
            if anchor != self.anchor {
                self.anchor = anchor;
                self.center_pending = true;
            }
        }
    }

    fn follow_current(&mut self, position: Duration) {
        self.cancel_drag();
        self.following = true;
        self.anchor = self.preview_anchor(position);
        self.center_pending = true;
    }

    fn browse(&mut self) {
        self.cancel_drag();
        self.following = false;
        self.center_pending = false;
    }

    pub fn center_anchor(&mut self) {
        if self.center_pending {
            if let Some(bounds) = self.anchor.and_then(|i| self.scroll.bounds_for_item(i + 1)) {
                self.scroll.set_offset(point(
                    px(0.0),
                    self.scroll.bounds().center().y - bounds.center().y,
                ));
                self.center_pending = false;
            }
        }
    }
}

fn timing_label(ms: u64) -> String {
    format!("{:02}:{:02}.{:03}", ms / 60_000, ms / 1000 % 60, ms % 1000)
}

fn timing_preview(
    editor: &TimingEditor,
    position: Duration,
    height: f32,
    text: Hsla,
    secondary: Hsla,
    accent: Hsla,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let lyrics = editor.draft.display_lyrics();
    let active = lyrics.display_line_index(position);
    let position_ms = position.as_millis().min(u128::from(u64::MAX)) as u64;
    v_flex()
        .id("timing-lyrics-scroll")
        .size_full()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&editor.scroll)
        .cursor(if editor.drag.is_some() {
            gpui_kit::CursorStyle::ClosedHand
        } else {
            gpui_kit::CursorStyle::OpenHand
        })
        .child(div().h(px(height)).flex_none())
        .children(lyrics.lines.iter().enumerate().map(|(index, line)| {
            let current = active == Some(index);
            let primary = if let Some(words) = line.word_timing.as_ref().filter(|_| current) {
                super::word_lyrics::WordLyrics::new(
                    words.clone(),
                    position_ms,
                    text.opacity(0.5),
                    accent,
                )
                .into_any_element()
            } else {
                div().child(line.text.clone()).into_any_element()
            };
            v_flex()
                .id(SharedString::from(format!("timing-row-{index}")))
                .w_full()
                .min_h(px(76.0))
                .flex_none()
                .justify_center()
                .gap_1()
                .px_6()
                .py_3()
                .text_center()
                .text_size(px(26.0))
                .line_height(relative(1.45))
                .font_weight(if current {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(if current { accent } else { text.opacity(0.6) })
                .child(primary)
                .when_some(
                    line.translation.clone().filter(|s| !s.is_empty()),
                    |this, translation| {
                        this.child(div().text_sm().text_color(secondary).child(translation))
                    },
                )
        }))
        .child(div().h(px(height)).flex_none())
}

impl NavidromeApp {
    fn sync_timing_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = &self.lyrics_ui.timing {
            let offset = editor.draft.offset_ms;
            self.lyrics_ui.offset.update(cx, |input, cx| {
                input.set_value(offset.to_string(), window, cx)
            });
        }
    }

    fn begin_timing_drag(&mut self, y: Pixels, cx: &mut Context<Self>) {
        if let Some(editor) = &mut self.lyrics_ui.timing {
            editor.begin_drag(y);
        }
        cx.notify();
    }

    fn move_timing_drag(
        &mut self,
        y: Pixels,
        finish: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .lyrics_ui
            .timing
            .as_ref()
            .is_none_or(|editor| editor.drag.is_none())
        {
            return;
        }
        let position = self.playback_for_display().position;
        if let Some(editor) = &mut self.lyrics_ui.timing {
            if finish {
                editor.finish_drag(y, position);
            } else {
                editor.update_drag(y);
            }
        }
        if finish {
            self.sync_timing_input(window, cx);
        }
        cx.notify();
    }

    fn save_lyrics_timing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &mut self.lyrics_ui.timing else {
            return;
        };
        editor.cancel_drag();
        let draft = editor.draft.clone();
        if self
            .state
            .lyrics
            .document
            .as_ref()
            .is_none_or(|d| d.timing_fingerprint() != editor.fingerprint)
            || self
                .state
                .now_playing
                .as_ref()
                .and_then(|song| self.lyrics_media(song))
                .as_ref()
                != self.lyrics_ui.media.as_ref()
        {
            self.close_lyrics_dialog(window, cx);
            return;
        }
        // Keep automatic content unpinned, exactly as the numeric offset setting.
        if !draft.temporary {
            let mut preference = draft.clone();
            if !draft.manual {
                preference.record = None;
            }
            if let Some(media) = &self.lyrics_ui.media {
                if let Err(error) = self.lyrics_cache.remember(media, &preference) {
                    self.notify_error(t!("Could not save lyrics: %{error}", error = error));
                    return;
                }
            }
        }
        self.state.lyrics.apply(draft);
        self.close_lyrics_dialog(window, cx);
    }

    pub(super) fn render_lyrics_timing_dialog(
        &self,
        dialog: Dialog,
        window: &Window,
        cx: &Context<Self>,
    ) -> Dialog {
        let Some(editor) = &self.lyrics_ui.timing else {
            return dialog;
        };
        let p = palette(cx);
        let lyrics = editor.draft.display_lyrics();
        let position = self.playback_for_display().position;
        let position_ms = position.as_millis().min(u128::from(u64::MAX)) as u64;
        let offset = editor.draft.offset_ms;
        let reserved = if editor.precision_open { 390.0 } else { 340.0 };
        let height = (f32::from(window.viewport_size().height) - reserved).clamp(120.0, 380.0);
        let preview = timing_preview(editor, position, height, p.text, p.text_secondary, p.brand)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui_kit::MouseDownEvent, _, cx| {
                    this.begin_timing_drag(event.position.y, cx);
                }),
            )
            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                if let Some(editor) = &mut this.lyrics_ui.timing {
                    editor.browse();
                }
                cx.notify();
            }));
        let reference = h_flex()
            .absolute()
            .left_0()
            .right_0()
            .top(px(height / 2.0))
            .justify_between()
            .child(div().w(px(12.0)).h(px(2.0)).bg(p.brand.opacity(0.6)))
            .child(div().w(px(12.0)).h(px(2.0)).bg(p.brand.opacity(0.6)));
        let content = v_flex().id("lyrics-timing-content").gap_3()
            .on_mouse_move(cx.listener(|this, event: &gpui_kit::MouseMoveEvent, window, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    this.move_timing_drag(event.position.y, false, window, cx);
                } else if let Some(editor) = &mut this.lyrics_ui.timing {
                    if editor.drag.is_some() { editor.cancel_drag(); cx.notify(); }
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, event: &gpui_kit::MouseUpEvent, window, cx| {
                this.move_timing_drag(event.position.y, true, window, cx);
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, event: &gpui_kit::MouseUpEvent, window, cx| {
                this.move_timing_drag(event.position.y, true, window, cx);
            }))
            .child(div().text_xs().text_color(p.text_secondary)
                .child(t!("Drag lyrics up to show them earlier, down to show them later. Scroll to browse without changing timing.")))
            .child(h_flex().justify_between().items_center()
                .child(div().text_sm().text_color(p.brand)
                    .child(t!("Playback reference: %{time}", time = timing_label(position_ms))))
                .child(Button::new("timing-follow-current").label(t!("Current line")).ghost().small()
                    .on_click(cx.listener(|this, _, _, cx| {
                        let position = this.playback_for_display().position;
                        if let Some(editor) = &mut this.lyrics_ui.timing {
                            editor.follow_current(position);
                        }
                        cx.notify();
                    }))))
            .child(div().relative().h(px(height)).overflow_hidden().border_1().border_color(p.border).rounded_md()
                .child(preview).child(reference))
            .when(lyrics.has_word_timing(), |this| this.child(div().text_xs().text_color(p.text_secondary)
                .child(t!("The highlight follows the current word. Drag anywhere in the lyrics to adjust the whole song."))))
            .child(h_flex().gap_2().items_center().justify_between()
                .child(Button::new("lyrics-earlier").label(t!("Earlier 0.1 s")).outline().on_click(cx.listener(|this, _, window, cx| this.adjust_lyrics_offset(-100, window, cx))))
                .child(div().text_sm().child(t!("Offset: %{seconds} s", seconds = format!("{:+.3}", offset as f64 / 1000.0))))
                .child(Button::new("lyrics-later").label(t!("Later 0.1 s")).outline().on_click(cx.listener(|this, _, window, cx| this.adjust_lyrics_offset(100, window, cx)))))
            .child(h_flex().gap_2().items_center()
                .child(Button::new("toggle-lyrics-precision").label(t!("Fine adjustment")).ghost().selected(editor.precision_open)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(editor) = &mut this.lyrics_ui.timing { editor.precision_open = !editor.precision_open; }
                        cx.notify();
                    })))
                .child(Button::new("undo-lyrics-timing").label(t!("Undo")).ghost().disabled(editor.undo.is_empty())
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(editor) = &mut this.lyrics_ui.timing {
                            editor.cancel_drag();
                            if let Some(before) = editor.undo.pop() { editor.draft = before; }
                        }
                        this.sync_timing_input(window, cx); cx.notify();
                    })))
                .child(Button::new("reset-lyrics-timing").label(t!("Reset")).ghost()
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(editor) = &mut this.lyrics_ui.timing {
                            editor.change(|document| { document.offset_ms = 0; document.timing_edits = None; });
                        }
                        this.sync_timing_input(window, cx); cx.notify();
                    }))))
            .when(editor.precision_open, |this| this
                .child(div().text_xs().text_color(p.text_secondary).child(t!("Positive values delay lyrics; negative values show them earlier. Only this song is affected.")))
                .child(h_flex().gap_2().items_center()
                    .child(Input::new(&self.lyrics_ui.offset).flex_1()).child(div().text_sm().child(t!("ms")))
                    .child(Button::new("apply-lyrics-offset").label(t!("Apply")).outline().on_click(cx.listener(|this, _, _, cx| this.apply_lyrics_offset_input(cx))))));
        dialog
            .title(t!("Lyrics timing"))
            .w(px(760.0))
            .max_w(px((f32::from(window.viewport_size().width) - 32.0).max(0.0)))
            .margin_top(px(16.0))
            .child(content)
            .footer(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("cancel-lyrics-timing")
                            .label(t!("Cancel"))
                            .ghost()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.close_lyrics_dialog(window, cx)
                            })),
                    )
                    .child(
                        Button::new("save-lyrics-timing")
                            .label(t!("Done"))
                            .primary()
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save_lyrics_timing(window, cx)
                            })),
                    ),
            )
    }

    pub(super) fn adjust_lyrics_offset(
        &mut self,
        delta: i64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(editor) = &mut self.lyrics_ui.timing {
            editor.change(|document| document.offset_ms = document.offset_ms.saturating_add(delta));
        }
        self.sync_timing_input(window, cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests;
