use gpui_kit::{AppContext as _, Bounds, Modifiers, Point, TestAppContext, VisualTestContext};

use super::*;
use gpui_kit::component::{Theme, ThemeMode};

struct SeekProbe {
    slider: Entity<SliderState>,
    disabled: bool,
    window_moves: usize,
}

impl Render for SeekProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .w(px(272.0))
            .h(px(96.0))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.window_moves += 1),
            )
            .child(mini_seek_control(
                &self.slider,
                self.disabled,
                0.0,
                hsla(0.9, 0.5, 0.5, 1.0),
                0.62,
            ))
    }
}

fn harness(
    cx: &mut TestAppContext,
    disabled: bool,
) -> (Entity<SeekProbe>, Entity<SliderState>, VisualTestContext) {
    cx.update(gpui_kit::init);
    let slider = cx.new(|_| SliderState::new());
    let mut probe = None;
    let slider_for_window = slider.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |window, cx| {
            let view = cx.new(|_| SeekProbe {
                slider: slider_for_window,
                disabled,
                window_moves: 0,
            });
            probe = Some(view.clone());
            cx.new(|cx| gpui_kit::component::Root::new(view, window, cx))
        })
        .unwrap()
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    (probe.unwrap(), slider, visual)
}

fn track_position(bounds: Bounds<Pixels>, fraction: f32) -> Point<Pixels> {
    // Deliberately hit above the visible 2px indicator but within its 16px track.
    point(
        bounds.left() + bounds.size.width * fraction,
        bounds.center().y - px(5.0),
    )
}

#[gpui_pre::test]
fn mini_seek_accepts_clicks_around_the_line_without_moving_the_window(cx: &mut TestAppContext) {
    let (probe, slider, mut visual) = harness(cx, false);
    let bounds = visual.update(|_, cx| slider.read(cx).bounds());
    visual.simulate_click(track_position(bounds, 0.75), Modifiers::default());
    visual.update(|_, cx| {
        assert!((slider.read(cx).value().end() - 75.0).abs() < 1.0);
        assert_eq!(probe.read(cx).window_moves, 0);
    });
}

#[gpui_pre::test]
fn mini_seek_drag_updates_the_shared_playback_slider(cx: &mut TestAppContext) {
    let (probe, slider, mut visual) = harness(cx, false);
    let bounds = visual.update(|_, cx| slider.read(cx).bounds());
    let start = track_position(bounds, 0.2);
    let middle = track_position(bounds, 0.5);
    let end = track_position(bounds, 0.9);
    visual.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    visual.simulate_mouse_move(middle, Some(MouseButton::Left), Modifiers::default());
    visual.simulate_mouse_move(end, Some(MouseButton::Left), Modifiers::default());
    visual.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
    visual.update(|_, cx| {
        assert!((slider.read(cx).value().end() - 90.0).abs() < 1.0);
        assert_eq!(probe.read(cx).window_moves, 0);
    });
}

#[gpui_pre::test]
fn mini_seek_is_inert_without_a_seekable_track(cx: &mut TestAppContext) {
    let (probe, slider, mut visual) = harness(cx, true);
    let bounds = visual.update(|_, cx| slider.read(cx).bounds());
    visual.simulate_click(track_position(bounds, 0.75), Modifiers::default());
    visual.update(|_, cx| {
        assert_eq!(slider.read(cx).value(), SliderValue::Single(0.0));
        assert_eq!(probe.read(cx).window_moves, 0);
    });
}

struct PlayProbe {
    playing: bool,
    has_track: bool,
    clicks: usize,
}

impl Render for PlayProbe {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex().w(px(168.0)).h(px(32.0)).child(mini_control_slot(
            mini_play_button(self.playing, self.has_track, cx).on_click(cx.listener(
                |this, _, _, cx| {
                    this.clicks += 1;
                    this.playing = !this.playing;
                    cx.notify();
                },
            )),
        ))
    }
}

#[gpui_pre::test]
fn mini_play_button_renders_and_handles_hover_clicks_and_disabled_states(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    for mode in [ThemeMode::Light, ThemeMode::Dark] {
        cx.update(|cx| {
            Theme::change(mode, None, cx);
            crate::app::apply_app_theme(cx);
        });
        for playing in [false, true] {
            for has_track in [false, true] {
                let mut probe = None;
                let window = cx.update(|cx| {
                    cx.open_window(Default::default(), |window, cx| {
                        let view = cx.new(|_| PlayProbe {
                            playing,
                            has_track,
                            clicks: 0,
                        });
                        probe = Some(view.clone());
                        cx.new(|cx| gpui_kit::component::Root::new(view, window, cx))
                    })
                    .unwrap()
                });
                let probe = probe.unwrap();
                let mut visual = VisualTestContext::from_window(window.into(), cx);
                visual.update(|window, cx| {
                    let _ = window.draw(cx);
                });
                let position = point(px(84.0), px(16.0));
                visual.simulate_mouse_move(position, None, Modifiers::default());
                visual.simulate_click(position, Modifiers::default());
                visual.update(|window, cx| {
                    let _ = window.draw(cx);
                    assert_eq!(probe.read(cx).clicks, usize::from(has_track));
                    assert_eq!(probe.read(cx).playing, playing ^ has_track);
                });
                visual.simulate_mouse_move(point(px(150.0), px(16.0)), None, Modifiers::default());
                visual.update(|window, cx| {
                    let _ = window.draw(cx);
                    window.remove_window();
                });
            }
        }
    }
}
