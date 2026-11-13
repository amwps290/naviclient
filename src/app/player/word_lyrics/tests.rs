use super::*;
use gpui_kit::component::Root;
use gpui_kit::{
    div, AppContext, Context, ParentElement, Render, Styled, TestAppContext, VisualTestContext,
};

struct Probe {
    width: f32,
    position: u64,
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let words = crate::lyrics::yrc::parse(
            "[1000,2000](1000,500,0)你 (1500,500,0)love (2000,1000,0)the world",
        )
        .lines
        .remove(0)
        .word_timing
        .unwrap();
        div()
            .w(px(self.width))
            .text_center()
            .text_size(px(24.0))
            .child(WordLyrics::new(
                words,
                self.position,
                gpui_kit::hsla(0.0, 0.0, 0.5, 1.0),
                gpui_kit::hsla(0.6, 1.0, 0.5, 1.0),
            ))
    }
}

#[gpui_pre::test]
fn word_drawing_survives_resize_pause_and_bidirectional_seeks(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let mut probe = None;
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            let view = cx.new(|_| Probe {
                width: 300.0,
                position: 1250,
            });
            probe = Some(view.clone());
            cx.new(|cx| Root::new(view, window, cx))
        })
        .unwrap()
    });
    let probe = probe.unwrap();
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    for (width, position) in [
        (300.0, 1250),
        (90.0, 2250),
        (90.0, 2250),
        (300.0, 999),
        (90.0, 3000),
    ] {
        visual.update(|window, cx| {
            probe.update(cx, |probe, cx| {
                probe.width = width;
                probe.position = position;
                cx.notify();
            });
            let _ = window.draw(cx);
        });
    }
    visual.update(|window, _| {
        let text: SharedString = "one two three four five".into();
        let style = window.text_style();
        let lines = window
            .text_system()
            .shape_text(
                text.clone(),
                px(20.0),
                &[style.to_run(text.len())],
                Some(px(80.0)),
                None,
            )
            .unwrap();
        let line = &lines[0];
        assert!(!line.wrap_boundaries.is_empty());
        let clips = segment_clips(
            line,
            0..text.len(),
            point(px(0.0), px(0.0)),
            px(80.0),
            px(30.0),
            TextAlign::Center,
        );
        assert_eq!(clips.len(), line.wrap_boundaries.len() + 1);
        assert!(clips
            .iter()
            .all(|clip| clip.origin.x >= px(0.0) && clip.right() <= px(80.0)));
        assert!(clips
            .windows(2)
            .all(|pair| pair[1].origin.y - pair[0].origin.y == px(30.0)));
    });
}
