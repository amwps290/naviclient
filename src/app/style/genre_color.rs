//! Name-derived color blocks for genre cards.

use gpui_kit::{rgb, Hsla};

const LIGHT_SWATCHES: [u32; 8] = [
    0xF3D9CF, 0xF2DDE3, 0xE3DDF0, 0xD8E3EF, 0xD6E8E2, 0xDFE8D2, 0xEFE6C9, 0xEBDCD0,
];

const DARK_SWATCHES: [u32; 8] = [
    0x4A3128, 0x4A2F39, 0x3A3249, 0x2C3A49, 0x2A403A, 0x35402A, 0x453D24, 0x43352A,
];

/// FNV-1a: unlike `DefaultHasher` it never changes between Rust releases, so a
/// genre keeps its color across updates.
fn swatch_index(name: &str) -> usize {
    let hash = name.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    hash as usize % LIGHT_SWATCHES.len()
}

pub(in crate::app) fn genre_swatch(name: &str, dark: bool) -> Hsla {
    let swatches = if dark { DARK_SWATCHES } else { LIGHT_SWATCHES };
    rgb(swatches[swatch_index(name)]).into()
}

#[cfg(test)]
mod tests;
