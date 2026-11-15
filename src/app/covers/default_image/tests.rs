use super::*;

#[test]
fn thumbnails_follow_device_resolution_and_bound_upscaling() {
    assert_eq!(device_side(128.0, 1.0), 128);
    assert_eq!(device_side(128.0, 1.25), 160);
    assert_eq!(device_side(96.0, 1.5), 144);
    assert_eq!(device_side(40.0, 2.0), 80);
    assert_eq!(device_side(0.0, 1.0), 1);
    assert_eq!(device_side(800.0, 2.0), 1024);
}

#[test]
fn strong_reduction_averages_detail_instead_of_sampling_one_texel() {
    let mut cache = DefaultCoverThumbnails::default();
    let checkerboard = RgbaImage::from_fn(64, 64, |x, y| {
        let value = if (x + y) % 2 == 0 { 0 } else { 255 };
        image::Rgba([value, value, value, 255])
    });
    cache.levels.insert(64, checkerboard);
    let image = cache.image(8).unwrap();
    assert_eq!(image.size(0).width.0, 8);
    for pixel in image.as_bytes(0).unwrap().chunks_exact(4) {
        assert!((125..=130).contains(&pixel[0]), "{pixel:?}");
        assert_eq!(pixel[3], 255);
    }
}

#[test]
fn baked_levels_preserve_opaque_artwork_and_render_bgra_colors() {
    let mut cache = DefaultCoverThumbnails::default();
    for &(side, bytes) in LEVELS {
        let original = image::load_from_memory(bytes).unwrap().to_rgba8();
        assert_eq!(original.dimensions(), (side, side));
        assert!(original.pixels().all(|pixel| pixel[3] == 255));
        let rendered = cache.image(side).unwrap();
        for (actual, expected) in rendered
            .as_bytes(0)
            .unwrap()
            .chunks_exact(4)
            .zip(original.pixels())
        {
            assert_eq!(actual, [expected[2], expected[1], expected[0], 255]);
        }
    }
}

#[test]
fn simultaneous_sizes_and_repeat_draws_reuse_their_own_textures() {
    let mut cache = DefaultCoverThumbnails::default();
    let small = cache.image(40).unwrap();
    let hero = cache.image(128).unwrap();
    assert_ne!(small.id, hero.id);
    assert!(cache.take_releases().is_empty());
    assert!(Arc::ptr_eq(&cache.image(40).unwrap(), &small));
    assert!(Arc::ptr_eq(&cache.image(128).unwrap(), &hero));
    assert_eq!(cache.image(40).unwrap().size(0).width.0, 40);
}

#[test]
fn releases_only_stale_sizes_and_keeps_current_scene_textures_alive() {
    let mut cache = DefaultCoverThumbnails::default();
    // An unusually varied scene can exceed the budget without invalidating an
    // image already painted earlier in that same scene.
    let oldest = cache.image(1).unwrap();
    for side in 2..=CAPACITY as u32 + 1 {
        cache.image(side).unwrap();
    }
    assert!(cache.take_releases().is_empty());
    // The next scene still needs its oldest thumbnail, protecting it from LRU.
    assert!(Arc::ptr_eq(&cache.image(1).unwrap(), &oldest));
    let released = cache.take_releases();
    assert_eq!(released.len(), 1);
    assert_ne!(released[0].id, oldest.id);
    assert_eq!(cache.images.len(), CAPACITY);
    let rerendered = cache.image(2).unwrap();
    assert_ne!(rerendered.id, released[0].id);
    let released = cache.take_releases();
    assert_eq!(released.len(), 1);
    assert_ne!(released[0].id, rerendered.id);
}
