//! Convert RGBA pixels to SNI ARGB32 network byte order.

pub(super) fn rgba_to_argb32(rgba: &[u8]) -> Vec<u8> {
    let mut argb = Vec::with_capacity(rgba.len());
    for pixel in rgba.chunks_exact(4) {
        argb.extend_from_slice(&[pixel[3], pixel[0], pixel[1], pixel[2]]);
    }
    argb
}

#[cfg(test)]
mod tests {
    use super::rgba_to_argb32;

    #[test]
    fn rgba_to_argb32_reorders_channels_per_pixel() {
        // 单像素 R=1 G=2 B=3 A=4 → A=4 R=1 G=2 B=3
        assert_eq!(rgba_to_argb32(&[1, 2, 3, 4]), vec![4, 1, 2, 3]);
        // 多像素按 4 字节分组，互不串扰
        assert_eq!(
            rgba_to_argb32(&[1, 2, 3, 4, 5, 6, 7, 8]),
            vec![4, 1, 2, 3, 8, 5, 6, 7]
        );
    }
}
