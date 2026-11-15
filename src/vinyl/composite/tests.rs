use gpui_kit::RenderImage;
use image::{Frame, Rgba, RgbaImage};

use super::{mix, rasterize};

#[test]
fn frame_blending_preserves_color_weights_and_opaque_edges() {
    let frame =
        |color| RenderImage::new(vec![Frame::new(RgbaImage::from_pixel(8, 8, Rgba(color)))]);
    let first = frame([0, 40, 200, 255]);
    let next = frame([200, 80, 0, 255]);
    let first = rasterize(&first, 8).unwrap();
    let next = rasterize(&next, 8).unwrap();
    for (blend, expected) in [
        (0.0, [0, 40, 200, 255]),
        (0.25, [50, 50, 150, 255]),
        (0.5, [100, 60, 100, 255]),
        (0.75, [150, 70, 50, 255]),
        (1.0, [200, 80, 0, 255]),
    ] {
        let pixels = mix(&first, &next, blend);
        assert!(pixels.pixels().all(|pixel| pixel.0 == expected));
    }
}

#[test]
fn mini_prefilter_removes_subpixel_checkerboard_without_transparent_rim() {
    let source = RgbaImage::from_fn(192, 192, |x, y| {
        let value = if (x + y) % 2 == 0 { 0 } else { 255 };
        Rgba([value, value, value, 255])
    });
    let frame = RenderImage::new(vec![Frame::new(source)]);
    let source = rasterize(&frame, 60).unwrap();
    let pixels = mix(&source, &source, 0.5);
    assert_eq!(pixels.dimensions(), (60, 60));
    for y in 5..55 {
        for x in 5..55 {
            let pixel = pixels[(x, y)];
            assert!((126..=129).contains(&pixel[0]), "{x},{y}: {pixel:?}");
            assert_eq!(pixel[0], pixel[1]);
            assert_eq!(pixel[0], pixel[2]);
        }
    }
    assert!(pixels.pixels().all(|pixel| pixel[3] == 255));
}
