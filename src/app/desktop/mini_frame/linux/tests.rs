use super::*;
use gpui_kit::{Bounds, Modifiers, ScaledPixels};
use gpui_pre::{size, TestAppContext, VisualTestContext};

struct ClientFrameFixture {
    mini: bool,
    clicks: usize,
}

impl Render for ClientFrameFixture {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // TestWindow has server decorations. Reproduce the kit's client frame
        // geometry to exercise its paint masks and hitboxes headlessly.
        let surface = div()
            .size_full()
            .p(px(20.0))
            .child(
                div()
                    .size_full()
                    .border_1()
                    .border_color(gpui_kit::red())
                    .child(
                        div()
                            .id("card")
                            .debug_selector(|| "card".to_string())
                            .size_full()
                            .rounded(px(14.0))
                            .bg(gpui_kit::blue())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.clicks += 1;
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element();
        if self.mini {
            clip_client_frame(surface, Edges::all(px(21.0)), window.viewport_size())
        } else {
            surface
        }
    }
}

fn border_is_visible(window: &Window) -> bool {
    window.painted_quads().iter().any(|quad| {
        if quad.border_color != gpui_kit::red() {
            return false;
        }
        let bounds = quad.bounds;
        let border = ScaledPixels(1.0 * window.scale_factor());
        let strips = [
            Bounds::new(bounds.origin, size(bounds.size.width, border)),
            Bounds::new(
                point(bounds.left(), bounds.bottom() - border),
                size(bounds.size.width, border),
            ),
            Bounds::new(bounds.origin, size(border, bounds.size.height)),
            Bounds::new(
                point(bounds.right() - border, bounds.top()),
                size(border, bounds.size.height),
            ),
        ];
        strips
            .iter()
            .any(|strip| !strip.intersect(&quad.content_mask.bounds).is_empty())
    })
}

#[gpui_pre::test]
fn mini_hides_the_square_frame_preserves_card_bounds_and_clicks_and_restores_on_exit(
    cx: &mut TestAppContext,
) {
    let (view, visual) = cx.add_window_view(|_, _| ClientFrameFixture {
        mini: false,
        clicks: 0,
    });
    let visual: &mut VisualTestContext = visual;
    visual.simulate_resize(size(px(312.0), px(136.0)));
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    let original = visual.debug_bounds("card").unwrap();
    assert_eq!(original.origin, point(px(21.0), px(21.0)));
    assert_eq!(original.size, size(px(270.0), px(94.0)));
    visual.update(|window, _| assert!(border_is_visible(window)));

    for round in 1..=3 {
        view.update(visual, |view, cx| {
            view.mini = true;
            cx.notify();
        });
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert_eq!(visual.debug_bounds("card").unwrap(), original);
        visual.update(|window, _| {
            assert!(!border_is_visible(window));
            let card = window
                .painted_quads()
                .into_iter()
                .find(|quad| quad.background.as_solid() == Some(gpui_kit::blue()))
                .unwrap();
            assert_ne!(card.corner_radii, Default::default());
            assert_eq!(card.content_mask.bounds, card.bounds);
        });
        visual.simulate_click(original.center(), Modifiers::default());
        assert_eq!(view.read_with(visual, |view, _| view.clicks), round);
        view.update(visual, |view, cx| {
            view.mini = false;
            cx.notify();
        });
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert_eq!(visual.debug_bounds("card").unwrap(), original);
        visual.update(|window, _| assert!(border_is_visible(window)));
    }
}
