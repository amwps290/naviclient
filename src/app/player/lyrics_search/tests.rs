//! Exercise real input events and the dialog's bubbled confirmation together.

use super::*;
use gpui_kit::component::Root;
use gpui_kit::{Focusable, TestAppContext, VisualTestContext};

struct Probe {
    ui: LyricsUi,
    entered: Vec<String>,
    closed: usize,
    _subscription: Subscription,
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full()
    }
}

fn harness(
    cx: &mut TestAppContext,
    kind: LyricsDialog,
    runtime: &Runtime,
) -> (Entity<Probe>, tokio::task::AbortHandle, VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        cx.set_reduce_motion(true);
    });
    let task = runtime.spawn(std::future::pending::<()>());
    let abort = task.abort_handle();
    let mut task = Some(task);
    let mut probe = None;
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            let entity = cx.new(|cx| {
                let mut ui = LyricsUi::new(window, cx);
                ui.dialog = Some(kind);
                ui.task = task.take();
                let input = match kind {
                    LyricsDialog::Search => ui.title.clone(),
                    LyricsDialog::Timing => ui.offset.clone(),
                };
                let subscription = cx.subscribe(&input, |this: &mut Probe, input, event, cx| {
                    if matches!(event, InputEvent::PressEnter { .. }) {
                        this.entered.push(input.read(cx).value().to_string());
                    }
                });
                Probe {
                    ui,
                    entered: Vec::new(),
                    closed: 0,
                    _subscription: subscription,
                }
            });
            probe = Some(entity.clone());
            cx.new(|cx| Root::new(entity, window, cx))
        })
        .unwrap()
    });
    let probe = probe.unwrap();
    let mut cx = VisualTestContext::from_window(window.into(), cx);
    cx.update(|window, cx| {
        let _ = window.draw(cx);
        let input = match kind {
            LyricsDialog::Search => probe.read(cx).ui.title.clone(),
            LyricsDialog::Timing => probe.read(cx).ui.offset.clone(),
        };
        let closing = probe.downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let closing = closing.clone();
            lyrics_dialog_behavior(dialog)
                .child(Input::new(&input))
                .on_close(move |_, _, cx| {
                    let _ = closing.update(cx, |this, _| {
                        this.closed += 1;
                        this.ui.cancel();
                    });
                })
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.update(|window, cx| {
        let ui = &probe.read(cx).ui;
        let input = match kind {
            LyricsDialog::Search => &ui.title,
            LyricsDialog::Timing => &ui.offset,
        };
        input.read(cx).focus_handle(cx).focus(window, cx);
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    (probe, abort, cx)
}

#[gpui_pre::test]
fn search_enter_keeps_dialog_and_task_alive_but_escape_cancels(cx: &mut TestAppContext) {
    let runtime = Runtime::new().unwrap();
    let (probe, abort, mut cx) = harness(cx, LyricsDialog::Search, &runtime);
    cx.simulate_keystrokes("t e s t enter");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(window.has_active_dialog(cx));
        assert_eq!(probe.read(cx).entered, ["test"]);
        assert_eq!(probe.read(cx).closed, 0);
        assert!(probe.read(cx).ui.task.is_some());
    });
    assert!(!abort.is_finished());
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(!window.has_active_dialog(cx));
        assert_eq!(probe.read(cx).closed, 1);
        assert!(probe.read(cx).ui.task.is_none());
    });
}

#[gpui_pre::test]
fn timing_enter_reaches_input_once_and_keeps_valid_and_invalid_values_open(
    cx: &mut TestAppContext,
) {
    let runtime = Runtime::new().unwrap();
    let (probe, _, mut cx) = harness(cx, LyricsDialog::Timing, &runtime);
    for value in ["750", "invalid"] {
        cx.update(|window, cx| {
            let input = probe.read(cx).ui.offset.clone();
            input.update(cx, |input, cx| input.set_value(value, window, cx));
            let _ = window.draw(cx);
        });
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        cx.update(|window, cx| assert!(window.has_active_dialog(cx)));
    }
    cx.update(|_, cx| {
        assert_eq!(probe.read(cx).entered, ["750", "invalid"]);
        assert_eq!(probe.read(cx).closed, 0);
    });
}
