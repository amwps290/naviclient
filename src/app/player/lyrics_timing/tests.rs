use super::*;

#[test]
fn cancelling_an_offset_drag_restores_the_draft_without_adding_undo_history() {
    let mut editor = TimingEditor::new(
        Document::native(crate::models::Lyrics::default()),
        Duration::ZERO,
    );
    editor.draft.offset_ms = 125;
    editor.begin_drag(px(20.0));
    editor.draft.offset_ms = 800;
    editor.scroll.set_offset(point(px(0.0), px(40.0)));
    editor.cancel_drag();
    assert_eq!(editor.draft.offset_ms, 125);
    assert_eq!(editor.scroll.offset().y, px(0.0));
    assert!(editor.undo.is_empty());
}

fn word_editor() -> TimingEditor {
    TimingEditor::new(
        Document::native(crate::lyrics::yrc::parse(
            "[1000,3000](1000,1000,0)你(2000,1000,0)好(3000,1000,0)呀\n[30000,1000](30000,1000,0)下句",
        )),
        Duration::from_millis(1500),
    )
}

#[test]
fn click_or_small_hand_movement_leaves_timing_and_history_unchanged() {
    let mut editor = word_editor();
    editor.begin_drag(px(100.0));
    editor.finish_drag(px(102.0), Duration::from_secs(2));
    assert_eq!(editor.draft.offset_ms, 0);
    assert!(editor.draft.timing_edits.is_none());
    assert!(editor.undo.is_empty());
    assert!(!editor.precision_open);
}

#[test]
fn drag_is_continuous_and_playback_advancing_or_large_cue_gaps_do_not_change_it() {
    let mut editor = word_editor();
    editor.draft.offset_ms = 125;
    editor.begin_drag(px(100.0));
    editor.update_drag(px(110.0));
    assert_eq!(editor.draft.offset_ms, 225);
    editor.sync_playback(Duration::from_secs(30));
    editor.update_drag(px(110.0));
    assert_eq!(editor.draft.offset_ms, 225);
    editor.update_drag(px(111.0));
    assert_eq!(editor.draft.offset_ms, 235);
    editor.finish_drag(px(111.0), Duration::from_secs(31));
    assert_eq!(editor.draft.offset_ms, 235);
    assert_eq!(editor.undo.len(), 1);
    assert!(!editor.center_pending);
}

#[test]
fn drag_shifts_all_words_equally_and_preserves_existing_word_corrections() {
    let mut editor = word_editor();
    editor.draft.set_word_start(0, 1, 2200);
    let corrections = editor.draft.timing_edits.clone();
    let before = editor.draft.display_lyrics();
    editor.begin_drag(px(100.0));
    editor.finish_drag(px(120.0), Duration::from_secs(2));
    let after = editor.draft.display_lyrics();
    assert_eq!(editor.draft.timing_edits, corrections);
    for (before, after) in before.lines.iter().zip(&after.lines) {
        assert_eq!(after.start_ms, before.start_ms.map(|start| start + 200));
        let before = before.word_timing.as_ref().unwrap();
        let after = after.word_timing.as_ref().unwrap();
        for (before, after) in before.segments.iter().zip(&after.segments) {
            assert_eq!(after.start_ms, before.start_ms + 200);
            assert_eq!(after.end_ms, before.end_ms.map(|end| end + 200));
        }
    }
}

#[test]
fn reversing_a_drag_uses_its_original_offset_and_can_be_undone_in_one_step() {
    let mut editor = word_editor();
    editor.draft.offset_ms = 500;
    editor.begin_drag(px(100.0));
    editor.update_drag(px(140.0));
    assert_eq!(editor.draft.offset_ms, 900);
    editor.finish_drag(px(80.0), Duration::from_secs(2));
    assert_eq!(editor.draft.offset_ms, 300);
    assert_eq!(editor.undo.len(), 1);
    editor.draft = editor.undo.pop().unwrap();
    assert_eq!(editor.draft.offset_ms, 500);
}

#[test]
fn browsing_disables_following_without_editing_timing_and_current_line_restores_it() {
    let mut editor = word_editor();
    editor.draft.offset_ms = 100;
    editor.browse();
    editor.sync_playback(Duration::from_secs(31));
    assert_eq!(editor.anchor, Some(0));
    assert_eq!(editor.draft.offset_ms, 100);
    editor.follow_current(Duration::from_secs(31));
    assert_eq!(editor.anchor, Some(1));
    assert!(editor.following);
    assert!(editor.center_pending);
}

struct PreviewProbe {
    editor: TimingEditor,
    position: Duration,
}

impl Render for PreviewProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.editor.sync_playback(self.position);
        self.editor.center_anchor();
        let text = hsla(0.0, 0.0, 0.8, 1.0);
        div()
            .id("timing-preview-probe")
            .w(px(500.0))
            .h(px(320.0))
            .on_mouse_move(
                cx.listener(|this, event: &gpui_kit::MouseMoveEvent, _, cx| {
                    if event.pressed_button == Some(MouseButton::Left) {
                        this.editor.update_drag(event.position.y);
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &gpui_kit::MouseUpEvent, _, cx| {
                    this.editor.finish_drag(event.position.y, this.position);
                    cx.notify();
                }),
            )
            .child(
                timing_preview(
                    &self.editor,
                    self.position,
                    320.0,
                    text,
                    text,
                    hsla(0.6, 1.0, 0.5, 1.0),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &gpui_kit::MouseDownEvent, _, cx| {
                        this.editor.begin_drag(event.position.y);
                        cx.notify();
                    }),
                ),
            )
    }
}

#[gpui_pre::test]
fn clicking_and_dragging_over_word_text_uses_the_whole_song_gesture(
    cx: &mut gpui_kit::TestAppContext,
) {
    use gpui_kit::component::Root;
    use gpui_kit::{Modifiers, VisualTestContext};

    cx.update(gpui_kit::init);
    let mut probe = None;
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            let entity = cx.new(|_| PreviewProbe {
                editor: word_editor(),
                position: Duration::from_millis(1500),
            });
            probe = Some(entity.clone());
            cx.new(|cx| Root::new(entity, window, cx))
        })
        .unwrap()
    });
    let probe = probe.unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    for _ in 0..2 {
        visual.update(|window, cx| {
            probe.update(cx, |_, cx| cx.notify());
            let _ = window.draw(cx);
        });
    }
    let position = visual.update(|_, cx| {
        let editor = &probe.read(cx).editor;
        let bounds = editor.scroll.bounds_for_item(1).unwrap();
        bounds.center() + editor.scroll.offset()
    });
    visual.simulate_mouse_down(position, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_up(position, MouseButton::Left, Modifiers::default());
    visual.update(|window, cx| {
        assert_eq!(probe.read(cx).editor.draft.offset_ms, 0);
        assert!(probe.read(cx).editor.undo.is_empty());
        let _ = window.draw(cx);
    });
    visual.simulate_mouse_down(position, MouseButton::Left, Modifiers::default());
    let moved = position + point(px(0.0), px(20.0));
    visual.simulate_mouse_move(moved, Some(MouseButton::Left), Modifiers::default());
    visual.simulate_mouse_up(moved, MouseButton::Left, Modifiers::default());
    visual.update(|window, cx| {
        let editor = &probe.read(cx).editor;
        assert_eq!(editor.draft.offset_ms, 200);
        assert!(editor.draft.timing_edits.is_none());
        assert_eq!(editor.undo.len(), 1);
        let _ = window.draw(cx);
    });
}
