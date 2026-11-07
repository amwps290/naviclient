use gpui_kit::Hsla;

use super::*;
use crate::app::covers::artwork::contrast_ratio;
use crate::app::style::tokens::{DARK, LIGHT};

#[test]
fn same_name_always_gets_the_same_color() {
    assert_eq!(genre_swatch("Rock", false), genre_swatch("Rock", false));
    assert_eq!(genre_swatch("摇滚", true), genre_swatch("摇滚", true));
}

#[test]
fn names_spread_across_the_swatches() {
    let names = [
        "Rock",
        "Pop",
        "Jazz",
        "Blues",
        "Folk",
        "Metal",
        "Classical",
        "Electronic",
        "Hip-Hop",
        "Country",
        "Soul",
        "Funk",
        "Reggae",
        "Punk",
        "摇滚",
        "流行",
        "民谣",
        "古典",
    ];
    let mut used = std::collections::HashSet::new();
    for name in names {
        used.insert(swatch_index(name));
    }
    assert!(used.len() >= 5, "only {} swatches used", used.len());
}

#[test]
fn every_swatch_is_readable() {
    let light_text = LIGHT.resolve();
    for swatch in LIGHT_SWATCHES {
        let color: Hsla = gpui_kit::rgb(swatch).into();
        assert!(contrast_ratio(light_text.text, color) >= 4.5);
        assert!(contrast_ratio(light_text.text_secondary, color) >= 4.5);
    }
    let dark_text = DARK.resolve();
    for swatch in DARK_SWATCHES {
        let color: Hsla = gpui_kit::rgb(swatch).into();
        assert!(contrast_ratio(dark_text.text, color) >= 4.5);
        assert!(contrast_ratio(dark_text.text_secondary, color) >= 4.5);
    }
}

#[test]
fn empty_name_does_not_panic() {
    let _ = genre_swatch("", false);
}
