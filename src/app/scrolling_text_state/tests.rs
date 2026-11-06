use super::*;
use gpui_kit::{font, px, FontFallbacks, FontFeatures, FontStyle, FontWeight};
use std::sync::Arc;

fn cached(cache: &mut TextWidthCache, text: &str, width: f32) -> Pixels {
    cache.measure(font("Font"), px(14.0), text.to_string().into(), || {
        px(width)
    })
}

fn hit(cache: &mut TextWidthCache, text: &str) -> Pixels {
    cache.measure(font("Font"), px(14.0), text.to_string().into(), || {
        panic!("cached text must not be reshaped")
    })
}

#[test]
fn font_family_weight_style_features_and_fallbacks_have_independent_measurements() {
    let mut cache = TextWidthCache::default();
    let base = font("Font");
    let fonts = [
        base.clone(),
        font("Other font"),
        Font {
            weight: FontWeight::BOLD,
            ..base.clone()
        },
        Font {
            style: FontStyle::Italic,
            ..base.clone()
        },
        Font {
            features: FontFeatures(Arc::new(vec![("liga".into(), 0)])),
            ..base.clone()
        },
        Font {
            fallbacks: Some(FontFallbacks(Arc::new(vec!["CJK".into()]))),
            ..base
        },
    ];
    for (index, font) in fonts.iter().enumerate() {
        let width = px(100.0 + index as f32);
        assert_eq!(
            cache.measure(font.clone(), px(14.0), "Same title".into(), || width),
            width
        );
    }
    for (index, font) in fonts.into_iter().enumerate() {
        assert_eq!(
            cache.measure(font, px(14.0), "Same title".into(), || panic!(
                "each font has a cached width"
            )),
            px(100.0 + index as f32),
        );
    }
    assert_eq!(cache.entries.len(), 6);
    assert_eq!(cache.recency.len(), 6);
}

#[test]
fn text_and_font_size_are_part_of_the_measurement_identity() {
    let mut cache = TextWidthCache::default();
    assert_eq!(cached(&mut cache, "First", 20.0), px(20.0));
    assert_eq!(cached(&mut cache, "Second", 30.0), px(30.0));
    assert_eq!(
        cache.measure(font("Font"), px(28.0), "First".into(), || px(40.0)),
        px(40.0)
    );
    assert_eq!(hit(&mut cache, "First"), px(20.0));
    assert_eq!(hit(&mut cache, "Second"), px(30.0));
    assert_eq!(cache.entries.len(), 3);
}

#[test]
fn eviction_keeps_recent_titles_instead_of_clearing_all_measurements() {
    let mut cache = TextWidthCache::with_limit(2);
    cached(&mut cache, "Frequent", 10.0);
    cached(&mut cache, "Old", 20.0);
    assert_eq!(hit(&mut cache, "Frequent"), px(10.0));
    cached(&mut cache, "New", 30.0);
    assert_eq!(hit(&mut cache, "Frequent"), px(10.0));
    assert_eq!(hit(&mut cache, "New"), px(30.0));
    assert_eq!(cached(&mut cache, "Old", 25.0), px(25.0));
    assert_eq!(cache.entries.len(), 2);
    assert_eq!(cache.recency.len(), 2);
}

#[test]
fn repeated_hits_and_new_titles_keep_both_cache_indexes_bounded() {
    let mut cache = TextWidthCache::with_limit(3);
    cached(&mut cache, "Frequent", 10.0);
    for index in 0..100 {
        assert_eq!(hit(&mut cache, "Frequent"), px(10.0));
        cached(&mut cache, &format!("Title {index}"), index as f32);
        assert!(cache.entries.len() <= 3);
        assert_eq!(cache.entries.len(), cache.recency.len());
    }
    assert_eq!(hit(&mut cache, "Frequent"), px(10.0));
    assert_eq!(TextWidthCache::default().limit, 20_000);
}

#[test]
fn invalid_measurements_cannot_poison_a_later_valid_measurement() {
    for width in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
        let mut cache = TextWidthCache::default();
        let result = cached(&mut cache, "Title", width);
        assert!(f32::from(result).is_nan() || f32::from(result) == width);
        assert!(cache.entries.is_empty());
        assert!(cache.recency.is_empty());
        assert_eq!(cached(&mut cache, "Title", 100.0), px(100.0));
        assert_eq!(hit(&mut cache, "Title"), px(100.0));
    }
}

#[test]
fn zero_capacity_measures_without_retaining_text() {
    let mut cache = TextWidthCache::with_limit(0);
    assert_eq!(cached(&mut cache, "Title", 10.0), px(10.0));
    assert_eq!(cached(&mut cache, "Title", 20.0), px(20.0));
    assert!(cache.entries.is_empty());
    assert!(cache.recency.is_empty());
}

#[test]
fn fitting_text_and_invalid_geometry_do_not_start_an_animation() {
    for (text, viewport) in [
        (0.0, 100.0),
        (99.0, 100.0),
        (100.0, 100.0),
        (200.0, 0.0),
        (200.0, -1.0),
        (f32::NAN, 100.0),
        (f32::INFINITY, 100.0),
        (200.0, f32::NAN),
        (200.0, f32::INFINITY),
        (f32::MAX, 100.0),
    ] {
        assert!(ScrollingTextMotion::for_widths(text, viewport).is_none());
    }
}

#[test]
fn scroll_timing_preserves_gap_speed_and_the_initial_hold() {
    let motion = ScrollingTextMotion::for_widths(280.0, 100.0).unwrap();
    assert_eq!(motion.gap, 32.0);
    assert_eq!(motion.travel, 312.0);
    let seconds = motion.period.as_secs_f32();
    assert!((seconds - (312.0 / 28.0 + 1.5)).abs() < 0.0001);
    assert!((seconds * motion.hold_fraction - 1.5).abs() < 0.0001);
    assert_eq!(motion.progress(0.0), 0.0);
    assert_eq!(motion.progress(motion.hold_fraction), 0.0);
    assert!((motion.progress((1.0 + motion.hold_fraction) / 2.0) - 0.5).abs() < 0.0001);
    assert_eq!(motion.progress(1.0), 1.0);
}

#[test]
fn short_overflow_keeps_the_four_second_minimum_period() {
    let motion = ScrollingTextMotion::for_widths(10.0, 5.0).unwrap();
    assert_eq!(motion.period, Duration::from_secs(4));
    assert_eq!(motion.progress(0.375), 0.0);
    assert!(motion.progress(0.4) > 0.0);
}

#[test]
fn invalid_or_out_of_range_frame_progress_cannot_move_text_outside_its_cycle() {
    let motion = ScrollingTextMotion::for_widths(280.0, 100.0).unwrap();
    for delta in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
        assert_eq!(motion.progress(delta), 0.0);
    }
    assert_eq!(motion.progress(2.0), 1.0);
}
