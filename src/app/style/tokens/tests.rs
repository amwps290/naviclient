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
    assert_readable(
        "brand hover on soft hover",
        p.brand_hover,
        p.brand_soft_hover,
    );
    assert_readable("text on soft", p.text, p.brand_soft);
    assert_readable("on-brand on brand", p.on_brand, p.brand);
    assert_readable("on-brand on hover", p.on_brand, rgb(hex.brand_hover).into());
    assert_readable(
        "on-brand on active",
        p.on_brand,
        rgb(hex.brand_active).into(),
    );
    for (name, bg) in [
        ("button_surface", hex.button_surface),
        ("button_surface_hover", hex.button_surface_hover),
        ("button_surface_active", hex.button_surface_active),
        ("button_tonal", hex.button_tonal),
        ("button_tonal_hover", hex.button_tonal_hover),
        ("button_tonal_active", hex.button_tonal_active),
    ] {
        assert_readable(&format!("text on {name}"), p.text, rgb(bg).into());
    }
    assert_readable(
        "on-danger on danger",
        rgb(hex.on_danger).into(),
        rgb(hex.danger).into(),
    );
    assert_readable(
        "on-danger on danger hover",
        rgb(hex.on_danger).into(),
        rgb(hex.danger_hover).into(),
    );
    assert_readable(
        "on-danger on danger active",
        rgb(hex.on_danger).into(),
        rgb(hex.danger_active).into(),
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
