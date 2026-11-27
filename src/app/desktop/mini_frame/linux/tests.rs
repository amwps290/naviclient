use super::*;
use gpui_kit::{BoxShadow, Corners, Modifiers, Tiling, WindowOptions};
use gpui_pre::{size, TestAppContext, VisualTestContext};

#[derive(Default)]
struct Card {
    clicks: usize,
}

impl Render for Card {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("card")
            .debug_selector(|| "card".to_string())
            .size_full()
            .rounded(px(29.0))
            .bg(gpui_kit::blue())
            .on_click(cx.listener(|this, _, _, cx| {
                this.clicks += 1;
                cx.notify();
            }))
    }
}

// Test windows report server decorations, so the kit won't draw its own frame.
// Put a rectangular shadow in the same decoration stage to verify the whole
// wrapper is skipped, rather than only clipping off part of its shadow.
#[derive(Clone, Default)]
struct RectangularFrame(std::rc::Rc<Cell<usize>>);

impl Render for RectangularFrame {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl RootPlugin for RectangularFrame {
    fn decorate(
        &self,
        surface: AnyElement,
        _: &Root,
        _: &mut Window,
        _: &mut App,
    ) -> impl IntoElement {
        let paints = self.0.clone();
        div()
            .relative()
            .size_full()
            .child(
                gpui_kit::canvas(
                    |_, _, _| (),
                    move |bounds, (), window, _| {
                        paints.set(paints.get() + 1);
                        window.paint_drop_shadows(
                            bounds,
                            Corners::default(),
                            &[BoxShadow {
                                color: hsla(0.0, 0.0, 0.0, 0.18),
                                blur_radius: px(10.0),
                                spread_radius: px(-1.0),
                                offset: point(px(0.0), px(2.0)),
                                inset: false,
                            }],
                        );
                    },
                )
                .absolute()
                .inset_0(),
            )
            .child(surface)
    }
}

#[gpui_pre::test]
fn mini_skips_the_frame_shadow_preserves_clicks_and_restores_the_frame(cx: &mut TestAppContext) {
    let (window, card) = cx.update(|cx| {
        register_before_kit(cx);
        gpui_kit::init(cx);
        Root::register_plugin(cx, |_, _| RectangularFrame::default());
        register_after_kit(cx);
        gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
            cx.new(|_| Card::default())
        })
        .unwrap()
    });
    let mut visual = VisualTestContext::from_window(window, cx);
    visual.simulate_resize(size(px(256.0), px(58.0)));
    let paints = visual.update(|window, cx| {
        let _ = window.draw(cx);
        Root::read(window, cx)
            .plugin::<RectangularFrame>()
            .unwrap()
            .read(cx)
            .0
            .clone()
    });
    assert!(paints.get() > 0);
    let original = visual.debug_bounds("card").unwrap();

    for round in 1..=3 {
        visual.update(|window, cx| set_mini(window, true, cx));
        let previous = paints.get();
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert_eq!(paints.get(), previous, "no rectangular shadow in mini mode");
        assert_eq!(visual.debug_bounds("card").unwrap(), original);
        visual.simulate_click(original.center(), Modifiers::default());
        assert_eq!(card.read_with(&visual, |card, _| card.clicks), round);

        visual.update(|window, cx| set_mini(window, false, cx));
        visual.update(|window, cx| {
            let _ = window.draw(cx);
        });
        assert!(paints.get() > previous, "regular frame resumes painting");
        assert_eq!(visual.debug_bounds("card").unwrap(), original);
    }
}

#[test]
fn mini_reserves_the_client_frame_space_only_on_non_tiled_edges() {
    assert_eq!(
        content_insets(Edges::all(px(20.0)), Tiling::default()),
        Edges::all(px(21.0)),
    );
    let tiling = Tiling {
        top: true,
        left: true,
        ..Tiling::default()
    };
    let paddings = Edges {
        top: px(0.0),
        left: px(0.0),
        right: px(20.0),
        bottom: px(20.0),
    };
    assert_eq!(
        content_insets(paddings, tiling),
        Edges {
            right: px(21.0),
            bottom: px(21.0),
            ..paddings
        },
    );
}
