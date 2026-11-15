//! 专辑封面和黑胶唱机视觉的渲染。

use crate::app::prelude::*;

impl NavidromeApp {
    pub(in crate::app) fn render_cover(
        &self,
        cover_id: Option<&str>,
        size: f32,
        _cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        self.render_cover_styled(cover_id, size, tokens::radius::COVER, Vec::new())
    }

    pub(in crate::app) fn render_cover_styled(
        &self,
        cover_id: Option<&str>,
        size: f32,
        radius: f32,
        shadows: Vec<gpui_kit::BoxShadow>,
    ) -> gpui_kit::AnyElement {
        let cover = cover_id
            .and_then(|id| self.browsing_cover_image(id))
            .cloned()
            .unwrap_or_else(|| self.default_cover.clone());
        self.mark_cover_image_used(&cover);
        img(cover)
            .size(px(size))
            // Override GPUI's intrinsic image ratio so portrait artwork cannot grow the slot.
            .aspect_square()
            .object_fit(ObjectFit::Cover)
            .flex_none()
            .rounded(px(radius))
            .shadow(shadows)
            .into_any_element()
    }

    pub(in crate::app) fn render_song_cover_styled(
        &self,
        song: &Song,
        size: f32,
        radius: f32,
        shadows: Vec<gpui_kit::BoxShadow>,
    ) -> gpui_kit::AnyElement {
        let cover = self
            .song_cover_image(song)
            .cloned()
            .unwrap_or_else(|| self.default_cover.clone());
        self.mark_cover_image_used(&cover);
        img(cover)
            .size(px(size))
            .aspect_square()
            .object_fit(ObjectFit::Cover)
            .flex_none()
            .rounded(px(radius))
            .shadow(shadows)
            .into_any_element()
    }

    pub(in crate::app) fn render_vinyl_record(
        &self,
        size: f32,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        let motion = self.turntable.motion.frame();
        let cover = if motion.outgoing {
            self.turntable.outgoing_cover.as_ref()
        } else {
            self.turntable.cover.as_ref()
        }
        .cloned()
        .unwrap_or_else(|| self.default_cover.clone());
        let rotation_phase = if motion.outgoing {
            self.turntable.outgoing_phase
        } else {
            self.vinyl_cover.phase()
        };
        let tonearm_engaged = motion.arm_degrees > 0.5;
        self.mark_cover_image_used(&cover);
        let label_size = size * 0.5;
        let label_offset = (size - label_size) * 0.5;
        let inner_cover_size = label_size - size * 0.018;
        let hole_size = size * 0.032;
        // Each tonearm layer uses (110, 110) in a 220px viewBox as its pivot.
        let tonearm_size = size * 0.94;
        let tonearm_pivot_x = size * 0.91;
        let tonearm_pivot_y = size * 0.015;
        let tonearm_left = tonearm_pivot_x - tonearm_size * 0.5;
        let tonearm_top = tonearm_pivot_y - tonearm_size * 0.5;
        let tonearm_base_size = size * 0.105;
        let metal_tint = self
            .now_playing_cover_palette()
            .map_or(palette(cx).text_muted, |(_, accent)| accent);
        let metal_body = hsla(0.1, 0.025, 0.78, 1.0).blend(metal_tint.opacity(0.06));
        let tonearm_layer = |icon: AppIcon, offset_x: f32, offset_y: f32, color: Hsla| {
            Icon::new(icon)
                .absolute()
                .left(px(tonearm_left + offset_x))
                .top(px(tonearm_top + offset_y))
                .with_size(px(tonearm_size))
                .text_color(color)
                .transform(Transformation::rotate(percentage(
                    motion.arm_degrees / 360.0,
                )))
        };
        let tonearm_base = div()
            .absolute()
            .left(px(tonearm_pivot_x - tonearm_base_size * 0.5))
            .top(px(tonearm_pivot_y - tonearm_base_size * 0.5))
            .size(px(tonearm_base_size))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(hsla(0.1, 0.025, 0.42, 0.5))
            .bg(linear_gradient(
                132.0,
                linear_color_stop(hsla(0.1, 0.025, 0.86, 1.0), 0.0),
                linear_color_stop(
                    hsla(0.1, 0.025, 0.48, 1.0).blend(metal_tint.opacity(0.06)),
                    1.0,
                ),
            ))
            .shadow(vec![box_shadow(
                px(0.0),
                px(size * 0.006),
                px(size * 0.018),
                px(-1.0),
                hsla(0.0, 0.0, 0.0, 0.24),
            )])
            .child(
                div()
                    .size(px(tonearm_base_size * 0.72))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_1()
                    .border_color(hsla(0.0, 0.0, 1.0, 0.24))
                    .bg(linear_gradient(
                        145.0,
                        linear_color_stop(hsla(0.1, 0.025, 0.42, 1.0), 0.0),
                        linear_color_stop(hsla(0.1, 0.025, 0.22, 1.0), 1.0),
                    ))
                    .child(
                        div()
                            .size(px(tonearm_base_size * 0.28))
                            .rounded_full()
                            .border_1()
                            .border_color(hsla(0.0, 0.0, 1.0, 0.2))
                            .bg(linear_gradient(
                                135.0,
                                linear_color_stop(hsla(0.1, 0.025, 0.82, 1.0), 0.0),
                                linear_color_stop(hsla(0.1, 0.025, 0.56, 1.0), 1.0),
                            )),
                    ),
            );
        let highlight = Icon::new(AppIcon::VinylHighlight)
            .absolute()
            .top_0()
            .left_0()
            .with_size(px(size))
            .text_color(hsla(0.0, 0.0, 1.0, 0.2))
            .transform(Transformation::rotate(percentage(rotation_phase)));

        // GPUI caches the SVG mask at this size; denser grooves still use one sprite.
        let grooves = Icon::new(AppIcon::VinylGrooves)
            .absolute()
            .top_0()
            .left_0()
            .with_size(px(size))
            .text_color(hsla(0.0, 0.0, 1.0, 0.12));

        let record = div()
            .absolute()
            .left(px(size * motion.record.x))
            .top(px(size * motion.record.y))
            .w(px(size))
            .h(px(size))
            .opacity(motion.record.opacity)
            .flex_none()
            .rounded_full()
            .bg(linear_gradient(
                132.0,
                linear_color_stop(hsla(0.0, 0.0, 0.115, 1.0), 0.0),
                linear_color_stop(hsla(0.0, 0.0, 0.025, 1.0), 1.0),
            ))
            .when(tonearm_engaged, |this| {
                this.shadow(vec![
                    box_shadow(
                        px(0.0),
                        px(size * 0.012),
                        px(size * 0.024),
                        px(0.0),
                        hsla(0.0, 0.0, 0.0, 0.36),
                    ),
                    box_shadow(
                        px(0.0),
                        px(size * 0.045),
                        px(size * 0.1),
                        px(-size * 0.02),
                        hsla(0.0, 0.0, 0.0, 0.24),
                    ),
                ])
            })
            .when(!tonearm_engaged, |this| {
                this.shadow(vec![
                    box_shadow(
                        px(0.0),
                        px(size * 0.008),
                        px(size * 0.015),
                        px(0.0),
                        hsla(0.0, 0.0, 0.0, 0.32),
                    ),
                    box_shadow(
                        px(0.0),
                        px(size * 0.035),
                        px(size * 0.08),
                        px(-size * 0.015),
                        hsla(0.0, 0.0, 0.0, 0.2),
                    ),
                ])
            })
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded_full()
                    .border_1()
                    .border_color(hsla(0.0, 0.0, 0.0, 0.5)),
            )
            .child(
                div()
                    .absolute()
                    .inset(px(size * 0.006))
                    .rounded_full()
                    .border_1()
                    .border_color(hsla(0.0, 0.0, 1.0, 0.09)),
            )
            .child(highlight)
            .child(grooves)
            .child(
                div()
                    .absolute()
                    .top(px(label_offset))
                    .left(px(label_offset))
                    .size(px(label_size))
                    .rounded_full()
                    .border_1()
                    .border_color(hsla(0.0, 0.0, 1.0, 0.14))
                    .bg(linear_gradient(
                        132.0,
                        linear_color_stop(
                            hsla(0.0, 0.0, 0.12, 1.0).blend(metal_tint.opacity(0.12)),
                            0.0,
                        ),
                        linear_color_stop(hsla(0.0, 0.0, 0.035, 1.0), 1.0),
                    ))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        if motion.outgoing {
                            self.vinyl_cover.render_frozen(inner_cover_size)
                        } else {
                            self.vinyl_cover.render(inner_cover_size, rotation_phase)
                        }
                        .unwrap_or_else(|| {
                            img(cover)
                                .size(px(inner_cover_size))
                                .aspect_square()
                                .object_fit(ObjectFit::Cover)
                                .flex_none()
                                .rounded_full()
                                .into_any_element()
                        }),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px((label_size - hole_size) * 0.5))
                            .left(px((label_size - hole_size) * 0.5))
                            .size(px(hole_size))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(linear_gradient(
                                132.0,
                                linear_color_stop(hsla(0.0, 0.0, 0.62, 1.0), 0.0),
                                linear_color_stop(hsla(0.0, 0.0, 0.14, 1.0), 1.0),
                            ))
                            .border_1()
                            .border_color(hsla(0.0, 0.0, 0.0, 0.65))
                            .child(
                                div()
                                    .size(px(size * 0.012))
                                    .rounded_full()
                                    .bg(hsla(0.0, 0.0, 0.025, 1.0)),
                            ),
                    ),
            );

        div()
            .relative()
            .size(px(size))
            .flex_none()
            .child(record)
            .child(tonearm_layer(
                AppIcon::Tonearm,
                size * 0.005,
                size * 0.008,
                hsla(0.0, 0.0, 0.0, 0.24),
            ))
            .child(tonearm_layer(AppIcon::Tonearm, 0.0, 0.0, metal_body))
            .child(tonearm_layer(
                AppIcon::TonearmShade,
                0.0,
                0.0,
                hsla(0.1, 0.025, 0.12, 0.94),
            ))
            .child(tonearm_layer(
                AppIcon::TonearmHighlight,
                0.0,
                0.0,
                hsla(0.0, 0.0, 1.0, 0.5),
            ))
            .child(tonearm_layer(
                AppIcon::TonearmStylus,
                size * 0.005,
                size * 0.008,
                hsla(0.0, 0.0, 0.0, 0.24),
            ))
            .child(tonearm_layer(
                AppIcon::TonearmStylus,
                0.0,
                0.0,
                hsla(0.0, 0.0, 1.0, 0.65),
            ))
            .child(tonearm_base)
            .into_any_element()
    }
}
