//! 生成精致的默认封面图（assets/default-cover.png）。
//! 运行：cargo run --bin gen_default_cover
//! 设计：对角渐变背景 + 双圆环（唱片沟槽感）+ 居中播放三角。

use image::{Rgba, RgbaImage};

fn blend(base: &mut [u8; 4], over: [u8; 4]) {
    let a = over[3] as f32 / 255.0;
    for i in 0..3 {
        base[i] = (base[i] as f32 * (1.0 - a) + over[i] as f32 * a) as u8;
    }
    base[3] = 255;
}

fn in_ring(x: f32, y: f32, cx: f32, cy: f32, r0: f32, r1: f32) -> bool {
    let d = ((x - cx) * (x - cx) + (y - cy) * (y - cy)).sqrt();
    d >= r0 && d <= r1
}

fn in_triangle(px: f32, py: f32, a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    let sign = |p1: (f32, f32), p2: (f32, f32), p3: (f32, f32)| {
        (p1.0 - p3.0) * (p2.1 - p3.1) - (p2.0 - p3.0) * (p1.1 - p3.1)
    };
    let d1 = sign((px, py), a, b);
    let d2 = sign((px, py), b, c);
    let d3 = sign((px, py), c, a);
    let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_neg && has_pos)
}

fn main() {
    let size = 512u32;
    let mut img = RgbaImage::new(size, size);

    // 对角渐变：左上深藏蓝 -> 右下深紫罗兰
    let top = [21.0f32, 27.0, 48.0];
    let bottom = [46.0f32, 24.0, 58.0];
    for y in 0..size {
        for x in 0..size {
            let t = ((x + y) as f32 / (2.0 * size as f32)).clamp(0.0, 1.0);
            let r = (top[0] + (bottom[0] - top[0]) * t) as u8;
            let g = (top[1] + (bottom[1] - top[1]) * t) as u8;
            let b = (top[2] + (bottom[2] - top[2]) * t) as u8;
            img.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }

    let cx = size as f32 / 2.0;
    let cy = size as f32 / 2.0;
    let white = |alpha: u8| [255u8, 255, 255, alpha];

    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f32, y as f32);
            let mut pixel = img.get_pixel(x, y).0;

            // 外环（唱片边缘感，微透明）
            if in_ring(px, py, cx, cy, 196.0, 203.0) {
                blend(&mut pixel, white(38));
            }
            // 内环（沟槽感）
            if in_ring(px, py, cx, cy, 154.0, 158.0) {
                blend(&mut pixel, white(24));
            }
            // 居中播放三角（指向右）
            if in_triangle(px, py, (214.0, 212.0), (214.0, 300.0), (310.0, 256.0)) {
                blend(&mut pixel, white(190));
            }
            // 三角边缘柔化：靠近三角形边界的外圈加一点低透明白
            if in_triangle(px, py, (208.0, 206.0), (208.0, 306.0), (318.0, 256.0))
                && !in_triangle(px, py, (214.0, 212.0), (214.0, 300.0), (310.0, 256.0))
            {
                blend(&mut pixel, white(28));
            }

            img.put_pixel(x, y, Rgba(pixel));
        }
    }

    img.save("assets/default-cover.png")
        .expect("failed to save default cover");
    println!("wrote assets/default-cover.png ({}x{size})", size);
}
