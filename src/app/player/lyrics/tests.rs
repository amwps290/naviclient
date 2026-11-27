use super::super::lyrics_state::LyricsState;
use super::*;
use crate::lyrics::Document;
use gpui_kit::component::Root;
use gpui_kit::{TestAppContext, VisualTestContext};

struct FollowProbe {
    lyrics: LyricsState,
    scroll: ScrollHandle,
    viewport_height: f32,
    row_height: f32,
}

impl Render for FollowProbe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("lyrics-follow-probe")
            .w(px(320.0))
            .h(px(self.viewport_height))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .child(div().h(px(240.0)).flex_none())
            .child(div().h(px(self.row_height)).flex_none().child("first"))
            .child(div().h(px(self.row_height)).flex_none().child("second"))
            .child(div().h(px(280.0)).flex_none())
    }
}

fn on_screen(cx: &mut TestAppContext) -> (Entity<FollowProbe>, VisualTestContext) {
    cx.update(gpui_kit::init);
    let mut probe = None;
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            let entity = cx.new(|_| {
                let mut lyrics = LyricsState::default();
                lyrics.apply(Document::native(crate::lyrics::lrc::parse(
                    "[00:01.00]first\n[00:05.00]second",
                )));
                lyrics.follow_reset = false;
                lyrics.active_index = Some(0);
                FollowProbe {
                    lyrics,
                    scroll: ScrollHandle::new(),
                    viewport_height: 240.0,
                    row_height: 64.0,
                }
            });
            probe = Some(entity.clone());
            cx.new(|cx| Root::new(entity, window, cx))
        })
        .unwrap()
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    (probe.unwrap(), visual)
}

#[gpui_pre::test]
fn cancelling_alignment_resumes_the_same_cue_without_resetting_the_viewport(
    cx: &mut TestAppContext,
) {
    let (probe, mut visual) = on_screen(cx);
    visual.update(|_, cx| {
        probe.update(cx, |probe, _| {
            let now = std::time::Instant::now();
            probe.scroll.set_offset(point(px(0.0), px(-40.0)));
            probe.lyrics.scroll_target = Some(px(-200.0));
            let before = probe.scroll.offset();
            follow_lyrics(&mut probe.lyrics, &probe.scroll, Some(0), true, now);
            assert_eq!(probe.scroll.offset(), before);
            assert_eq!(probe.lyrics.active_index, Some(0));
            let target = probe.scroll.bounds().center().y
                - probe.scroll.bounds_for_item(1).unwrap().center().y;
            follow_lyrics(&mut probe.lyrics, &probe.scroll, Some(0), false, now);
            assert_eq!(probe.lyrics.scroll_target, Some(target));
            assert_ne!(probe.scroll.offset(), before);
            assert_ne!(probe.scroll.offset().y, px(0.0));
            assert!(!probe.lyrics.follow_reset);
            for _ in 0..40 {
                follow_lyrics(&mut probe.lyrics, &probe.scroll, Some(0), false, now);
            }
            assert_eq!(probe.lyrics.scroll_target, None);
            assert_eq!(probe.scroll.offset().y, target);
        });
    });
}

#[gpui_pre::test]
fn leaving_alignment_follows_the_latest_cue(cx: &mut TestAppContext) {
    let (probe, mut visual) = on_screen(cx);
    visual.update(|_, cx| {
        probe.update(cx, |probe, _| {
            let now = std::time::Instant::now();
            probe.scroll.set_offset(point(px(0.0), px(-40.0)));
            follow_lyrics(&mut probe.lyrics, &probe.scroll, Some(1), true, now);
            assert_eq!(probe.lyrics.active_index, Some(0));
            let target = probe.scroll.bounds().center().y
                - probe.scroll.bounds_for_item(2).unwrap().center().y;
            follow_lyrics(&mut probe.lyrics, &probe.scroll, Some(1), false, now);
            assert_eq!(probe.lyrics.active_index, Some(1));
            assert_eq!(probe.lyrics.scroll_target, Some(target));
        });
    });
}

#[gpui_pre::test]
fn manual_scroll_pauses_auto_follow_until_cooldown_or_explicit_resume(cx: &mut TestAppContext) {
    let (probe, mut visual) = on_screen(cx);
    visual.update(|_, cx| {
        probe.update(cx, |probe, _| {
            let start = std::time::Instant::now();
            probe.scroll.set_offset(point(px(0.0), px(-80.0)));
            probe.lyrics.pause_follow_for_scroll(start);
            follow_lyrics(
                &mut probe.lyrics,
                &probe.scroll,
                Some(1),
                false,
                start + Duration::from_secs(2),
            );
            assert_eq!(probe.scroll.offset().y, px(-80.0));
            assert_eq!(probe.lyrics.scroll_target, None);

            // Once cooldown expires, following automatically resumes to the active line.
            let target = probe.scroll.bounds().center().y
                - probe.scroll.bounds_for_item(2).unwrap().center().y;
            follow_lyrics(
                &mut probe.lyrics,
                &probe.scroll,
                Some(1),
                false,
                start + super::super::lyrics_state::MANUAL_SCROLL_PAUSE,
            );
            assert_eq!(probe.lyrics.scroll_target, Some(target));
            assert_eq!(probe.lyrics.manual_scroll_until, None);

            // Explicit resume cancels cooldown immediately.
            probe.lyrics.pause_follow_for_scroll(start);
            probe.lyrics.resume_follow_now();
            follow_lyrics(&mut probe.lyrics, &probe.scroll, Some(1), false, start);
            assert_eq!(probe.lyrics.scroll_target, Some(target));
        });
    });
}

#[gpui_pre::test]
fn switching_lyric_viewports_recenters_the_same_cue_with_its_new_height(cx: &mut TestAppContext) {
    let (probe, mut visual) = on_screen(cx);
    let now = std::time::Instant::now();
    let old_scroll = visual.update(|_, cx| {
        probe.update(cx, |probe, cx| {
            probe.lyrics.follow_resume = true;
            for _ in 0..40 {
                follow_lyrics(&mut probe.lyrics, &probe.scroll, Some(0), false, now);
            }
            let old = probe.scroll.clone();
            probe.scroll = ScrollHandle::new();
            probe.viewport_height = 360.0;
            probe.row_height = 160.0;
            probe.lyrics.follow_resume = true;
            cx.notify();
            old
        })
    });
    let old_offset = old_scroll.offset();
    visual.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
        probe.update(cx, |probe, _| {
            let target = probe.scroll.bounds().center().y
                - probe.scroll.bounds_for_item(1).unwrap().center().y;
            assert_ne!(target, old_offset.y);
            for _ in 0..40 {
                follow_lyrics(&mut probe.lyrics, &probe.scroll, Some(0), false, now);
            }
            assert_eq!(probe.lyrics.active_index, Some(0));
            assert_eq!(probe.lyrics.scroll_target, None);
            assert_eq!(probe.scroll.offset().y, target);
            assert_eq!(old_scroll.offset(), old_offset);
        });
    });
}
