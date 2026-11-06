//! Scrolling labels with font-aware measurement and shared motion planning.

use super::scrolling_text_state::ScrollingTextMotion;
use super::*;

impl NavidromeApp {
    pub(super) fn render_scrolling_title(
        &self,
        text: SharedString,
        viewport_width: f32,
        id: SharedString,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        self.render_scrolling_text(
            text,
            viewport_width,
            rems(0.875).to_pixels(window.rem_size()),
            FontWeight::MEDIUM,
            id,
            window,
        )
    }

    pub(super) fn render_scrolling_text(
        &self,
        text: SharedString,
        viewport_width: f32,
        font_size: Pixels,
        weight: FontWeight,
        id: SharedString,
        window: &mut Window,
    ) -> gpui_kit::AnyElement {
        let text_style = window.text_style().highlight(weight);
        let text_width = self.text_width_cache.borrow_mut().measure(
            text_style.font(),
            font_size,
            text.clone(),
            || {
                window
                    .text_system()
                    .shape_line(
                        text.clone(),
                        font_size,
                        &[text_style.to_run(text.len())],
                        None,
                    )
                    .width
            },
        );
        let viewport_width = if viewport_width.is_finite() {
            viewport_width.max(0.0)
        } else {
            0.0
        };

        let Some(motion) = ScrollingTextMotion::for_widths(f32::from(text_width), viewport_width)
        else {
            return div()
                .w(px(viewport_width))
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(font_size)
                .font_weight(weight)
                .child(text)
                .into_any_element();
        };

        let animation = Animation::new(motion.period)
            .repeat()
            .with_easing(move |delta| motion.progress(delta));
        let repeated_text = text.clone();

        div()
            .w(px(viewport_width))
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .child(
                h_flex()
                    .flex_none()
                    .gap(px(motion.gap))
                    .text_size(font_size)
                    .font_weight(weight)
                    .child(text)
                    .child(repeated_text)
                    .with_animation(id, animation, move |this, delta| {
                        this.ml(px(-motion.travel * delta))
                    }),
            )
            .into_any_element()
    }
}
