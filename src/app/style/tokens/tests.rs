use super::*;
use crate::app::covers::artwork::contrast_ratio;

const AA: f32 = 4.5;

fn assert_readable(name: &str, foreground: Hsla, background: Hsla) {
    let ratio = contrast_ratio(foreground, background);
    assert!(ratio >= AA, "{name} is {ratio:.2}:1");
}

fn assert_palette(hex: PaletteHex) {
    let p = hex.resolve();
    for (surface_name, surface) in [("page", p.page), ("raised", p.raised), ("sunken", p.sunken)] {
        assert_readable(&format!("text on {surface_name}"), p.text, surface);
        assert_readable(
            &format!("secondary on {surface_name}"),
            p.text_secondary,
            surface,
        );
        assert_readable(&format!("muted on {surface_name}"), p.text_muted, surface);
        assert_readable(&format!("brand on {surface_name}"), p.brand, surface);
    }
    assert_readable("brand on soft", p.brand, p.brand_soft);
    assert_readable("text on soft", p.text, p.brand_soft);
    assert_readable("on-brand on brand", p.on_brand, p.brand);
    assert_readable("on-brand on hover", p.on_brand, rgb(hex.brand_hover).into());
    assert_readable(
        "on-brand on active",
        p.on_brand,
        rgb(hex.brand_active).into(),
    );
}

#[test]
fn light_palette_meets_aa() {
    assert_palette(LIGHT);
}

#[test]
fn dark_palette_meets_aa() {
    assert_palette(DARK);
}
