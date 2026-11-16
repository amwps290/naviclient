use super::*;
use gpui_pre::{size, TestAppContext, VisualTestContext, WindowBounds, WindowOptions};

struct RoundedCard;

impl Render for RoundedCard {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .rounded(px(16.0))
            .bg(hsla(0.9, 0.3, 0.7, 1.0))
    }
}

fn square_backgrounds(window: &Window) -> Vec<Hsla> {
    let size = window.viewport_size().scale(window.scale_factor());
    window
        .painted_quads()
        .into_iter()
        .filter(|quad| {
            quad.bounds.size == size
                && quad.corner_radii == Default::default()
                && !quad.background.is_transparent()
        })
        .filter_map(|quad| quad.background.as_solid())
        .collect()
}

#[gpui_pre::test]
fn mini_corners_have_no_square_background_and_exit_restores_the_current_theme(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::app::init_window_frame(cx);
    });
    let (window, _) = cx.update(|cx| {
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(gpui_pre::Bounds::new(
                    point(px(0.0), px(0.0)),
                    size(px(272.0), px(96.0)),
                ))),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(|_| RoundedCard),
        )
        .unwrap()
    });
    let mut visual = VisualTestContext::from_window(window, cx);
    // A headless window verifies the kit's painted Root, while the native
    // background appearance is supplied by the platform in the application.
    for (initial, changed) in [
        (ThemeMode::Light, ThemeMode::Dark),
        (ThemeMode::Dark, ThemeMode::Light),
    ] {
        visual.update(|window, cx| {
            Theme::change(initial, Some(window), cx);
            crate::app::apply_app_theme(cx);
            let _ = window.draw(cx);
            assert_eq!(square_backgrounds(window), vec![cx.theme().background]);

            set_mini_window_surface(window, true, cx);
            crate::app::desktop::mini_frame::MiniWindowFrame::default().set_mini(window, true, cx);
        });
        visual.update(|window, cx| {
            let _ = window.draw(cx);
            assert!(square_backgrounds(window).is_empty());
            assert!(window.painted_quads().iter().any(|quad| {
                quad.corner_radii != Default::default() && !quad.background.is_transparent()
            }));
            Theme::change(changed, Some(window), cx);
            crate::app::apply_app_theme(cx);
        });
        visual.update(|window, cx| {
            let _ = window.draw(cx);
            assert!(square_backgrounds(window).is_empty());
            set_mini_window_surface(window, false, cx);
            crate::app::desktop::mini_frame::MiniWindowFrame::default().set_mini(window, false, cx);
        });
        visual.update(|window, cx| {
            let _ = window.draw(cx);
            assert_eq!(square_backgrounds(window), vec![cx.theme().background]);
        });
    }
}
