use super::*;
use gpui_kit::{ImageFormat, TestAppContext};
use image::{Rgba, RgbaImage};

#[cfg(target_os = "linux")]
mod native;

fn render_image(pixels: RgbaImage) -> Arc<RenderImage> {
    Arc::new(RenderImage::new(vec![Frame::new(pixels)]))
}

fn source_image() -> Arc<Image> {
    let pixels = RgbaImage::from_fn(64, 64, |x, y| {
        let value = if (x + y) % 2 == 0 { 0 } else { 255 };
        Rgba([value, value, value, 255])
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    pixels
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    Arc::new(Image::from_bytes(ImageFormat::Png, bytes.into_inner()))
}

#[test]
fn video_thumbnails_keep_wide_aspect_and_do_not_upscale_or_mutate_sources() {
    for (width, height, expected) in [(1600, 900, (160, 90)), (80, 160, (80, 45)), (1, 1, (1, 1))] {
        let source = render_image(RgbaImage::from_pixel(
            width,
            height,
            Rgba([190, 80, 15, 127]),
        ));
        let output = filter_wide(&source, 160).unwrap();
        assert_eq!(output.buffer().dimensions(), expected);
        assert!(output
            .buffer()
            .pixels()
            .all(|pixel| pixel.0 == [190, 80, 15, 127]));
        assert_eq!(source.size(0).width.0, width as i32);
        assert_eq!(source.size(0).height.0, height as i32);
    }
}

#[test]
fn square_and_video_versions_never_share_a_thumbnail_key() {
    let square = Key {
        source: 42,
        side: 160,
        wide: false,
    };
    let video = Key {
        wide: true,
        ..square
    };
    assert!(square != video);
    assert_eq!(HashSet::from([square, video]).len(), 2);
}

#[test]
fn strong_reduction_filters_fine_patterns_without_mutating_the_original() {
    let source = render_image(RgbaImage::from_fn(1024, 1024, |x, y| {
        let value = if (x + y) % 2 == 0 { 0 } else { 255 };
        Rgba([value, value, value, 255])
    }));
    let output = filter(&source, 40).unwrap();
    assert_eq!(output.buffer().dimensions(), (40, 40));
    for pixel in output.buffer().pixels() {
        assert!((125..=130).contains(&pixel[0]), "{pixel:?}");
        assert_eq!(pixel[3], 255);
    }
    assert_eq!(source.size(0).width.0, 1024);
    assert_eq!(
        &source.as_bytes(0).unwrap()[..8],
        &[0, 0, 0, 255, 255, 255, 255, 255]
    );
}

#[test]
fn center_crop_matches_cover_fit_and_preserves_bgra_and_alpha_without_upscaling() {
    for (width, height) in [(7, 4), (4, 7)] {
        let source = render_image(RgbaImage::from_fn(width, height, |x, y| {
            let coordinate = if width > height { x } else { y };
            if (2..6).contains(&coordinate) {
                Rgba([190, 80, 15, 127])
            } else {
                Rgba([0, 0, 0, 255])
            }
        }));
        let output = filter(&source, 128).unwrap();
        assert_eq!(output.buffer().dimensions(), (4, 4));
        assert!(output
            .buffer()
            .pixels()
            .all(|pixel| pixel.0 == [190, 80, 15, 127]));
    }
}

struct Probe {
    source: Arc<Image>,
    cache: Rc<RefCell<CoverThumbnails>>,
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        FilteredCover {
            source: self.source.clone(),
            cache: self.cache.clone(),
            size: 16.0,
            radius: 3.0,
            shadows: Vec::new(),
        }
    }
}

#[gpui_pre::test]
fn background_completion_reuses_textures_and_updates_for_display_scale(cx: &mut TestAppContext) {
    let runner = cx.clone();
    let source = source_image();
    let cache = Rc::new(RefCell::new(CoverThumbnails::default()));
    let (_, visual) = cx.add_window_view(|_, _| Probe {
        source: source.clone(),
        cache: cache.clone(),
    });
    visual.simulate_scale_factor_change(1.0);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    runner.run_until_parked();
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    runner.run_until_parked();
    let key = Key {
        source: source.id(),
        side: 16,
        wide: false,
    };
    let first = cache.borrow().entries[&key].image.clone().unwrap();
    assert_eq!(first.size(0).width.0, 16);
    visual.update(|window, cx| {
        let again = CoverThumbnails::image(&cache, &source, 16, window, cx).unwrap();
        assert!(Arc::ptr_eq(&first, &again));
    });
    visual.simulate_scale_factor_change(2.0);
    visual.update(|window, cx| {
        let _ = window.draw(cx);
    });
    runner.run_until_parked();
    let key = Key {
        source: source.id(),
        side: 32,
        wide: false,
    };
    assert_eq!(
        cache.borrow().entries[&key]
            .image
            .as_ref()
            .unwrap()
            .size(0)
            .width
            .0,
        32
    );

    // Retirement also removes a job that has not delivered its result yet.
    visual.update(|window, cx| {
        assert!(CoverThumbnails::image(&cache, &source, 24, window, cx).is_none());
        cache.borrow_mut().retire(source.id());
    });
    runner.run_until_parked();
    assert!(cache.borrow().entries.is_empty());
    assert_eq!(cache.borrow().bytes, 0);
    assert_eq!(cache.borrow_mut().take_releases().len(), 2);
}

fn insert(cache: &mut CoverThumbnails, source: u64, side: u32) -> Key {
    let key = Key {
        source,
        side,
        wide: false,
    };
    let image = render_image(RgbaImage::new(side, side));
    cache.bytes += image.as_bytes(0).unwrap().len();
    cache.entries.insert(
        key,
        Entry {
            image: Some(image),
            _task: Task::ready(()),
        },
    );
    cache.recent.push_front(key);
    cache.active.insert(key);
    key
}

#[test]
fn byte_budget_keeps_scene_textures_and_releases_only_unused_sizes() {
    let mut cache = CoverThumbnails::default();
    let keys: Vec<_> = (0..9)
        .map(|source| insert(&mut cache, source, 1024))
        .collect();
    assert!(cache.bytes > BYTE_BUDGET);
    assert!(cache.take_releases().is_empty());
    // The oldest image remains in use; eviction skips it and releases the next.
    cache.active.insert(keys[0]);
    let released = cache.take_releases();
    assert_eq!(released.len(), 1);
    assert!(cache.entries.contains_key(&keys[0]));
    assert!(!cache.entries.contains_key(&keys[1]));
    assert_eq!(cache.bytes, BYTE_BUDGET);
    cache.retire(0);
    assert_eq!(cache.take_releases().len(), 1);
    assert_eq!(cache.bytes, BYTE_BUDGET - 1024 * 1024 * 4);
}

#[test]
fn entry_budget_bounds_small_thumbnails_independently_of_bytes() {
    let mut cache = CoverThumbnails::default();
    for source in 0..=CAPACITY as u64 {
        insert(&mut cache, source, 1);
    }
    assert!(cache.take_releases().is_empty());
    assert_eq!(cache.take_releases().len(), 1);
    assert_eq!(cache.entries.len(), CAPACITY);
}
