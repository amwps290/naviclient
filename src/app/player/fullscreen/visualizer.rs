//! Symmetrical rounded-capsule waveform painted from the real output envelope.

use crate::app::prelude::*;
use crate::audio::visualizer::WAVEFORM_BINS;

pub(super) fn waveform_view(frame: [f32; WAVEFORM_BINS], color: Hsla) -> gpui_kit::AnyElement {
    gpui_kit::canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let width = f32::from(bounds.size.width);
            let height = f32::from(bounds.size.height);
            if width <= 1.0 || height <= 1.0 {
                return;
            }
            let center_y = bounds.origin.y + px(height * 0.5);
            let mut baseline = gpui_kit::PathBuilder::stroke(px(0.75));
            baseline.move_to(point(bounds.origin.x, center_y));
            baseline.line_to(point(bounds.origin.x + px(width), center_y));
            if let Ok(path) = baseline.build() {
                window.paint_path(path, color.opacity(0.14));
            }
            let silent = frame.iter().all(|value| *value <= 0.001);
            let count = WAVEFORM_BINS;
            let step = width / count as f32;
            let bar_width = (step * 0.46).clamp(1.5, 3.2);
            let radius = bar_width * 0.5;
            for (index, raw) in frame.iter().enumerate() {
                let t = index as f32 / (count - 1).max(1) as f32;
                let edge_taper = (t * std::f32::consts::PI).sin().powf(0.45);
                let energy = if silent {
                    0.0
                } else {
                    raw.clamp(0.0, 1.0).powf(0.62) * edge_taper
                };
                let bar_height = if silent {
                    2.0
                } else {
                    (energy * height * 0.88).max(2.5)
                };
                let x = bounds.origin.x + px(index as f32 * step + (step - bar_width) * 0.5);
                let y = bounds.origin.y + px((height - bar_height) * 0.5);
                let opacity = if silent { 0.18 } else { 0.26 + 0.62 * energy };
                window.paint_quad(
                    gpui_kit::fill(
                        gpui_kit::Bounds::new(
                            point(x, y),
                            gpui_kit::size(px(bar_width), px(bar_height)),
                        ),
                        color.opacity(opacity),
                    )
                    .corner_radii(px(radius)),
                );
            }
        },
    )
    .w_full()
    .h(px(34.0))
    .into_any_element()
}
