use std::io::Cursor;
use std::sync::atomic::AtomicBool;

use image::{ImageFormat, Rgba, RgbaImage};

use super::{build_frame, cover_pixels, rotate, SIDE};

#[test]
fn rotation_is_clockwise_and_keeps_center() {
    let mut source = RgbaImage::from_pixel(5, 5, Rgba([0, 0, 0, 255]));
    source[(2, 0)] = Rgba([10, 20, 30, 255]);
    source[(2, 2)] = Rgba([40, 50, 60, 255]);
    let rotated = rotate(&source, 0.25);
    assert_eq!(rotated[(4, 2)], source[(2, 0)]);
    assert_eq!(rotated[(2, 2)], source[(2, 2)]);
    assert_eq!(rotate(&source, 0.0)[(2, 0)], source[(2, 0)]);
}

#[test]
fn rotation_keeps_opaque_square_corners_for_later_filtering_and_clipping() {
    let color = Rgba([30, 80, 170, 255]);
    let source = RgbaImage::from_pixel(32, 32, color);
    for phase in [0.0, 0.125, 0.25, 0.75] {
        assert!(rotate(&source, phase).pixels().all(|pixel| *pixel == color));
    }
}

#[test]
fn portrait_and_landscape_are_center_cropped_and_bgra() {
    for (width, height) in [(12, 4), (4, 12)] {
        let mut source = RgbaImage::from_pixel(width, height, Rgba([255, 0, 0, 255]));
        for y in (height - 4) / 2..(height + 4) / 2 {
            for x in (width - 4) / 2..(width + 4) / 2 {
                source[(x, y)] = Rgba([10, 20, 100, 255]);
            }
        }
        let mut bytes = Cursor::new(Vec::new());
        source.write_to(&mut bytes, ImageFormat::Png).unwrap();
        let pixels = cover_pixels(bytes.get_ref()).unwrap();
        assert_eq!(pixels.dimensions(), (SIDE, SIDE));
        assert_eq!(pixels[(SIDE / 2, SIDE / 2)].0, [100, 20, 10, 255]);
        assert_eq!(pixels[(SIDE / 4, SIDE / 4)].0, [100, 20, 10, 255]);
    }
}

#[test]
fn cancelled_work_skips_even_decoding() {
    assert!(build_frame(&[], &AtomicBool::new(true)).unwrap().is_none());
}

#[test]
fn transparent_cover_is_flattened_to_opaque_black_before_rotation() {
    let source = RgbaImage::from_pixel(4, 4, Rgba([10, 20, 100, 128]));
    let mut bytes = Cursor::new(Vec::new());
    source.write_to(&mut bytes, ImageFormat::Png).unwrap();
    let pixels = cover_pixels(bytes.get_ref()).unwrap();
    assert!(pixels.pixels().all(|pixel| pixel.0 == [50, 10, 5, 255]));
}

#[test]
fn prepares_only_one_unrotated_source() {
    let source = RgbaImage::from_pixel(4, 4, Rgba([10, 20, 30, 255]));
    let mut bytes = Cursor::new(Vec::new());
    source.write_to(&mut bytes, ImageFormat::Png).unwrap();
    let frame = build_frame(bytes.get_ref(), &AtomicBool::new(false))
        .unwrap()
        .unwrap();
    assert_eq!(frame.frame_count(), 1);
    assert_eq!(frame.as_bytes(0).unwrap().len(), (SIDE * SIDE * 4) as usize);
    assert!(frame
        .as_bytes(0)
        .unwrap()
        .chunks_exact(4)
        .all(|pixel| pixel == [30, 20, 10, 255]));
}
