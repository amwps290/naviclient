//! Optional desktop probe using the production filtering widget and renderer.

use super::*;
use gpui_kit::{size, WindowBounds, WindowDecorations, WindowOptions};
use smol::Timer;
use std::time::Duration;

struct Comparison {
    source: Arc<Image>,
    cache: Rc<RefCell<CoverThumbnails>>,
}

impl Render for Comparison {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut content = v_flex().size_full().p_6().gap_6().child(format!(
            "Bilibili cover filtering / scale {}",
            window.scale_factor()
        ));
        for size in [180.0, 40.0] {
            content = content.child(
                h_flex()
                    .gap_6()
                    .child(
                        v_flex()
                            .w(px(220.0))
                            .gap_2()
                            .child(format!("Original / {size}px"))
                            .child(
                                img(self.source.clone())
                                    .size(px(size))
                                    .aspect_square()
                                    .object_fit(ObjectFit::Cover)
                                    .rounded(px(12.0)),
                            ),
                    )
                    .child(
                        v_flex()
                            .w(px(220.0))
                            .gap_2()
                            .child(format!("Filtered / {size}px"))
                            .child(FilteredCover {
                                source: self.source.clone(),
                                cache: self.cache.clone(),
                                size,
                                radius: 12.0,
                                shadows: Vec::new(),
                            }),
                    ),
            );
        }
        content.text_color(cx.theme().foreground)
    }
}

#[test]
#[ignore = "requires a Linux desktop and NAVICLIENT_COVER_VISUAL_FIXTURE"]
fn native_cover_filtering_comparison() {
    let path =
        std::env::var("NAVICLIENT_COVER_VISUAL_FIXTURE").expect("provide a local cover fixture");
    let bytes = std::fs::read(path).unwrap();
    let source = Arc::new(Image::from_bytes(ImageFormat::Jpeg, bytes));
    gpui_kit::application()
        .with_assets(crate::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let cache = Rc::new(RefCell::new(CoverThumbnails::default()));
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::centered(size(px(560.0), px(450.0)), cx)),
                    window_decorations: Some(WindowDecorations::Client),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    window.set_window_title("Naviclient Cover Filtering Probe");
                    window.activate_window();
                    cx.new(|_| Comparison {
                        source,
                        cache: cache.clone(),
                    })
                },
            )
            .unwrap();
            cx.spawn(async move |cx| {
                Timer::after(Duration::from_secs(45)).await;
                cx.update(|cx| {
                    assert_eq!(cache.borrow().entries.len(), 2);
                    assert!(cache
                        .borrow()
                        .entries
                        .values()
                        .all(|entry| entry.image.is_some()));
                    println!("native cover filtering: both display sizes prepared");
                    cx.quit();
                });
            })
            .detach();
        });
}
