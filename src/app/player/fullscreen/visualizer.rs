//! A small waveform painted from the real output envelope, with no synthetic motion.

use crate::app::prelude::*;
use crate::audio::visualizer::WAVEFORM_BINS;

pub(super) fn waveform_view(frame: [f32; WAVEFORM_BINS], color: Hsla) -> gpui_kit::AnyElement {
    gpui_kit::canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let width = f32::from(bounds.size.width);
            let height = f32::from(bounds.size.height);
            let center = bounds.origin.y + px(height * 0.5);
            let mut baseline = gpui_kit::PathBuilder::stroke(px(0.75));
            baseline.move_to(point(bounds.origin.x, center));
            baseline.line_to(point(bounds.origin.x + px(width), center));
            if let Ok(path) = baseline.build() {
                window.paint_path(path, color.opacity(0.18));
            }
            if frame.iter().all(|value| *value <= 0.001) {
                return;
            }
            for direction in [-1.0, 1.0] {
                let mut path = gpui_kit::PathBuilder::stroke(px(1.0));
                path.move_to(point(bounds.origin.x, center));
                for (index, value) in frame.iter().enumerate() {
                    let x = width * index as f32 / (WAVEFORM_BINS - 1) as f32;
                    let amplitude = value.clamp(0.0, 1.0).powf(0.65) * height * 0.43;
                    path.line_to(point(
                        bounds.origin.x + px(x),
                        center + px(direction * amplitude),
                    ));
                }
                path.line_to(point(bounds.origin.x + px(width), center));
                if let Ok(path) = path.build() {
                    window.paint_path(
                        path,
                        color.opacity(if direction < 0.0 { 0.65 } else { 0.32 }),
                    );
                }
            }
        },
    )
    .w_full()
    .h(px(36.0))
    .into_any_element()
}
