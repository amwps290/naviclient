use super::*;

fn key(scale: f32) -> Key {
    Key {
        width: (272.0 * scale).round() as u32,
        height: (96.0 * scale).round() as u32,
        scale,
        tint: (
            gpui_kit::rgb(0xE4DDD1).into(),
            gpui_kit::rgb(0xDADCE7).into(),
        ),
        border: gpui_kit::hsla(0.0, 0.0, 1.0, 0.22),
    }
}

#[test]
fn all_four_corners_have_symmetric_transparent_coverage_at_each_dpi() {
    for scale in [1.0, 1.25, 1.5, 2.0] {
        let key = key(scale);
        let pixels = rasterize(key);
        assert_eq!(pixels[(0, 0)][3], 0);
        assert_eq!(pixels[(key.width / 2, 0)][3], 255);
        assert_eq!(pixels[(0, key.height / 2)][3], 255);
        let mut fractional = 0;
        for y in 0..(RADIUS * scale).ceil() as u32 {
            for x in 0..(RADIUS * scale).ceil() as u32 {
                let alpha = pixels[(x, y)][3];
                assert_eq!(alpha, pixels[(key.width - 1 - x, y)][3]);
                assert_eq!(alpha, pixels[(x, key.height - 1 - y)][3]);
                assert_eq!(alpha, pixels[(key.width - 1 - x, key.height - 1 - y)][3]);
                fractional += usize::from(alpha > 0 && alpha < 255);
            }
        }
        assert!(fractional > 16, "curves need a graduated alpha edge");
    }
}

#[test]
fn border_and_background_share_coverage_without_darkening_edge_rgb() {
    let mut key = key(1.0);
    key.tint = (
        gpui_kit::rgb(0xF0C080).into(),
        gpui_kit::rgb(0xF0C080).into(),
    );
    key.border = gpui_kit::transparent_black();
    let plain = rasterize(key);
    key.border = gpui_kit::hsla(0.0, 0.0, 1.0, 0.8);
    let bordered = rasterize(key);
    for (plain, bordered) in plain.pixels().zip(bordered.pixels()) {
        assert_eq!(
            plain[3], bordered[3],
            "a border cannot increase outline opacity"
        );
        assert_eq!(
            &plain.0[..3],
            &[0x80, 0xC0, 0xF0],
            "atlas pixels must use straight alpha"
        );
    }
}

#[gpui_pre::test]
fn unchanged_frames_reuse_the_texture_and_changes_release_the_old_one(
    cx: &mut gpui_kit::TestAppContext,
) {
    cx.update(gpui_kit::init);
    let visual = cx.add_empty_window();
    visual.update(|window, _| {
        let key = key(1.0);
        let mut cache = MiniSurface::default();
        let first = cache.image(
            key.width, key.height, key.scale, key.tint, key.border, window,
        );
        let again = cache.image(
            key.width, key.height, key.scale, key.tint, key.border, window,
        );
        assert!(Arc::ptr_eq(&first, &again));
        let changed = cache.image(key.width, key.height, 1.25, key.tint, key.border, window);
        assert_ne!(first.id, changed.id);
        cache.release(window);
        assert!(cache.cached.is_none());
    });
}
