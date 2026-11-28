use super::*;

#[test]
fn extracts_palette_from_default_cover() {
    let palette = extract_cover_palette(include_bytes!("../../../../assets/default-cover.png"));
    assert!(palette.is_some());
}

#[test]
fn cover_request_needed_never_burns_an_id_against_a_palette_revisit() {
    // 网格/队列预取：已有图像、不需要调色板 → 不请求。
    assert!(!cover_request_needed(true, false, false));
    assert!(!cover_request_needed(true, true, false));
    // 预取过的封面在播放时必须能补提调色板（历史 bug：只增不减的
    // 请求集合把 id 永久“烧掉”，播放页/侧边栏动态配色拿不到数据）。
    assert!(cover_request_needed(true, false, true));
    // 调色板已在 → 不重复请求。
    assert!(!cover_request_needed(true, true, true));
    // 图像还没有 → 总要请求。
    assert!(cover_request_needed(false, false, false));
    assert!(cover_request_needed(false, false, true));
    assert!(cover_request_needed(false, true, false));
}

#[test]
fn normalize_cover_bytes_downscales_oversized_images_and_preserves_small_ones() {
    use image::{ImageFormat, Rgb, RgbImage};
    use std::io::Cursor;

    // 1920x1080 wide cover (like Bilibili) should be downscaled to <= MAX_COVER_EDGE.
    let large = RgbImage::from_fn(1920, 1080, |x, y| {
        Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8])
    });
    let mut large_bytes = Vec::new();
    large
        .write_to(&mut Cursor::new(&mut large_bytes), ImageFormat::Png)
        .unwrap();
    let normalized = normalize_cover_bytes(large_bytes.clone());
    assert!(normalized.len() < large_bytes.len());
    let (w, h) = image::ImageReader::new(Cursor::new(&normalized))
        .with_guessed_format()
        .unwrap()
        .into_dimensions()
        .unwrap();
    assert_eq!(w, MAX_COVER_EDGE);
    assert!(h <= MAX_COVER_EDGE && h > 0);

    // 256x256 cover is already within budget and should pass through untouched.
    let small = RgbImage::from_pixel(256, 256, Rgb([42, 84, 126]));
    let mut small_bytes = Vec::new();
    small
        .write_to(&mut Cursor::new(&mut small_bytes), ImageFormat::Png)
        .unwrap();
    let untouched = normalize_cover_bytes(small_bytes.clone());
    assert_eq!(untouched, small_bytes);
}
