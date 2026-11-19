use gpui_kit::RenderImage;
use image::{Frame, Rgba, RgbaImage};

use super::{rasterize, rotate};

#[test]
fn a_midpoint_angle_keeps_one_sharp_contour_instead_of_two_ghosts() {
    let source = RgbaImage::from_fn(192, 192, |x, _| {
        let value = if x < 96 { 0 } else { 255 };
        Rgba([value, value, value, 255])
    });
    // The old 0°/3° crossfade spread this edge over four or five pixels.
    // At 1.5°, geometric sampling must keep a single subpixel transition.
    let pixels = rotate(&source, 1.5 / 360.0);
    for y in [20, 40, 150, 170] {
        let row: Vec<_> = (80..112).map(|x| pixels[(x, y)][0]).collect();
        assert!(row.windows(2).all(|pair| pair[0] <= pair[1]));
        assert_eq!(
            row.iter()
                .filter(|&&value| value > 0 && value < 255)
                .count(),
            1
        );
    }
    assert!(pixels.pixels().all(|pixel| pixel[3] == 255));
}

#[test]
fn mini_prefilter_removes_subpixel_checkerboard_without_transparent_rim() {
    let source = RgbaImage::from_fn(192, 192, |x, y| {
        let value = if (x + y) % 2 == 0 { 0 } else { 255 };
        Rgba([value, value, value, 255])
    });
    let frame = RenderImage::new(vec![Frame::new(source)]);
    let source = rasterize(&frame, 60).unwrap();
    let pixels = rotate(&source, 1.5 / 360.0);
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

#[test]
fn rotation_remains_continuous_across_old_frame_boundaries_and_a_full_turn() {
    let source = RgbaImage::from_fn(192, 192, |x, y| Rgba([x as u8, y as u8, 100, 255]));
    for degrees in [3.0, 90.0, 180.0, 360.0] {
        let before = rotate(&source, (degrees - 0.001) / 360.0);
        let after = rotate(&source, (degrees + 0.001) / 360.0);
        for y in 35..157 {
            for x in 35..157 {
                for channel in 0..3 {
                    assert!(before[(x, y)][channel].abs_diff(after[(x, y)][channel]) <= 1);
                }
            }
        }
    }
}
