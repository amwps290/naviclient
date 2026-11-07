use super::*;

#[test]
fn large_window_uses_full_size_hero() {
    let sizes = hero_sizes(1280.0, 820.0, false);
    assert_eq!(sizes.cover, 220.0);
    assert_eq!(sizes.title, 40.0);
    assert_eq!(hero_sizes(1280.0, 820.0, true).cover, 150.0);
}

#[test]
fn narrow_or_short_window_shrinks_the_hero() {
    for (width, height) in [(1039.0, 820.0), (1280.0, 759.0), (900.0, 600.0)] {
        let sizes = hero_sizes(width, height, false);
        assert_eq!(sizes.cover, 140.0, "{width}x{height}");
        assert_eq!(sizes.title, 28.0, "{width}x{height}");
        assert_eq!(hero_sizes(width, height, true).cover, 110.0);
    }
}

#[test]
fn exact_thresholds_stay_full_size() {
    assert_eq!(hero_sizes(1040.0, 760.0, false).cover, 220.0);
}
