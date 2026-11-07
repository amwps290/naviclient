use gpui_kit::hsla;

use super::*;
use crate::app::covers::artwork::contrast_ratio;
use crate::app::style::tokens::{DARK, LIGHT};

const SURFACES: [TintSurface; 4] = [
    TintSurface::HomeFeature,
    TintSurface::DetailHeader,
    TintSurface::NowPlaying,
    TintSurface::MiniPlayer,
];

#[test]
fn text_stays_readable_on_every_cover_color() {
    for (palette, dark) in [(LIGHT.resolve(), false), (DARK.resolve(), true)] {
        for surface in SURFACES {
            for hue in (0..36).map(|step| step as f32 / 36.0) {
                for saturation in [0.0, 0.3, 0.6, 1.0] {
                    for lightness in [0.02, 0.5, 0.98] {
                        let color = hsla(hue, saturation, lightness, 1.0);
                        let (start, end) =
                            cover_tint(surface, Some((color, color)), palette.page, dark).unwrap();
                        for background in [start, end] {
                            for text in [palette.text, palette.text_secondary] {
                                let ratio = contrast_ratio(text, background);
                                assert!(
                                    ratio >= 4.5,
                                    "{surface:?} dark={dark} h={hue} s={saturation} l={lightness}: {ratio:.2}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn missing_palette_produces_no_tint() {
    let page = LIGHT.resolve().page;
    assert!(cover_tint(TintSurface::DetailHeader, None, page, false).is_none());
    assert!(cover_tint(TintSurface::NowPlaying, None, page, true).is_none());
}

#[test]
fn hue_survives_normalization() {
    let page = LIGHT.resolve().page;
    let red = hsla(0.0, 1.0, 0.05, 1.0);
    let (start, _) = cover_tint(TintSurface::NowPlaying, Some((red, red)), page, false).unwrap();
    let rgb = start.to_rgb();
    assert!(rgb.r > rgb.g && rgb.r > rgb.b);
}

#[test]
fn grey_covers_stay_neutral() {
    let page = LIGHT.resolve().page;
    let grey = hsla(0.0, 0.0, 0.5, 1.0);
    let (start, _) = cover_tint(TintSurface::NowPlaying, Some((grey, grey)), page, false).unwrap();
    let (tinted, base) = (start.to_rgb(), page.to_rgb());
    assert!(tinted.r < base.r);
}
