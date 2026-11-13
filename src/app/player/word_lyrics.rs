//! Full-text shaping with playback-driven, per-segment foreground clipping.

use std::{cell::RefCell, rc::Rc};

use gpui_kit::{
    point, px, size, App, AvailableSpace, Bounds, ContentMask, Element, ElementId, GlobalElementId,
    Hsla, InspectorElementId, IntoElement, LayoutId, Pixels, SharedString, TextAlign, WhiteSpace,
    Window, WrappedLine,
};

use crate::models::WordTimedLine;

pub(in crate::app) struct WordLyrics {
    words: WordTimedLine,
    position_ms: u64,
    base_color: Hsla,
    accent_color: Hsla,
}

pub(in crate::app) struct Layout {
    base: Vec<WrappedLine>,
    accent: Vec<WrappedLine>,
    line_height: Pixels,
    align: TextAlign,
}

impl WordLyrics {
    pub(in crate::app) fn new(
        words: WordTimedLine,
        position_ms: u64,
        base_color: Hsla,
        accent_color: Hsla,
    ) -> Self {
        Self {
            words,
            position_ms,
            base_color,
            accent_color,
        }
    }
}

impl IntoElement for WordLyrics {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for WordLyrics {
    type RequestLayoutState = Rc<RefCell<Option<Layout>>>;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let layout = Rc::new(RefCell::new(None));
        let state = layout.clone();
        let text: SharedString = self.words.text.clone().into();
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.pixel_snap(style.line_height_in_pixels(window.rem_size()));
        let mut base_run = style.to_run(text.len());
        base_run.color = self.base_color;
        let mut accent_run = base_run.clone();
        accent_run.color = self.accent_color;
        let id = window.request_measured_layout(
            Default::default(),
            move |known, available, window, _| {
                let width = if style.white_space == WhiteSpace::Normal {
                    known.width.or(match available.width {
                        AvailableSpace::Definite(width) => Some(width),
                        _ => None,
                    })
                } else {
                    None
                };
                // GPUI caches shaping by text, font metrics and wrapping width;
                // changing only the decoration color reuses those glyph layouts.
                let base = window
                    .text_system()
                    .shape_text(
                        text.clone(),
                        font_size,
                        std::slice::from_ref(&base_run),
                        width,
                        None,
                    )
                    .unwrap_or_default();
                let accent = window
                    .text_system()
                    .shape_text(
                        text.clone(),
                        font_size,
                        std::slice::from_ref(&accent_run),
                        width,
                        None,
                    )
                    .unwrap_or_default();
                let measured = size(
                    base.iter()
                        .map(|line| line.width())
                        .max()
                        .unwrap_or(px(0.0)),
                    base.iter().map(|line| line.size(line_height).height).sum(),
                );
                *state.borrow_mut() = Some(Layout {
                    base: base.into_vec(),
                    accent: accent.into_vec(),
                    line_height,
                    align: style.text_align,
                });
                measured
            },
        );
        (id, layout)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let state = state.borrow();
        let Some(layout) = state.as_ref() else {
            return;
        };
        let mut origin = bounds.origin;
        let mut byte_offset = 0;
        for (base, accent) in layout.base.iter().zip(&layout.accent) {
            if let Err(error) = base.paint(
                origin,
                layout.line_height,
                layout.align,
                Some(bounds),
                window,
                cx,
            ) {
                log::warn!("could not paint word lyrics: {error}");
                return;
            }
            for segment in &self.words.segments {
                let progress = segment.progress(self.position_ms);
                if progress <= 0.0 {
                    continue;
                }
                let range = segment.text_range.start.saturating_sub(byte_offset)
                    ..segment
                        .text_range
                        .end
                        .saturating_sub(byte_offset)
                        .min(base.len());
                if segment.text_range.start >= byte_offset + base.len() || range.is_empty() {
                    continue;
                }
                let clips = segment_clips(
                    base,
                    range,
                    origin,
                    bounds.size.width,
                    layout.line_height,
                    layout.align,
                );
                let mut remaining = clips
                    .iter()
                    .map(|clip| f32::from(clip.size.width))
                    .sum::<f32>()
                    * progress;
                for mut clip in clips {
                    let width = f32::from(clip.size.width).min(remaining);
                    remaining -= width;
                    if width <= 0.0 {
                        break;
                    }
                    clip.size.width = px(width);
                    window.with_content_mask(Some(ContentMask { bounds: clip }), |window| {
                        if let Err(error) = accent.paint(
                            origin,
                            layout.line_height,
                            layout.align,
                            Some(bounds),
                            window,
                            cx,
                        ) {
                            log::warn!("could not paint word lyric highlight: {error}");
                        }
                    });
                }
            }
            origin.y += base.size(layout.line_height).height;
            byte_offset += base.len() + 1;
        }
    }
}

fn segment_clips(
    line: &WrappedLine,
    range: std::ops::Range<usize>,
    origin: gpui_kit::Point<Pixels>,
    width: Pixels,
    height: Pixels,
    align: TextAlign,
) -> Vec<Bounds<Pixels>> {
    let layout = &line.unwrapped_layout;
    let boundaries = line
        .wrap_boundaries
        .iter()
        .map(|boundary| {
            let glyph = &layout.runs[boundary.run_ix].glyphs[boundary.glyph_ix];
            (glyph.index, glyph.position.x)
        })
        .chain([(line.len(), layout.width)]);
    let mut clips = Vec::new();
    let mut start_byte = 0;
    let mut start_x = px(0.0);
    for (row, (end_byte, end_x)) in boundaries.enumerate() {
        let begin = range.start.max(start_byte);
        let end = range.end.min(end_byte);
        if begin < end {
            // Keep grapheme/ligature clusters intact when a source range falls
            // inside a shaped glyph. Most providers time entire words/syllables.
            let cluster = layout
                .runs
                .iter()
                .flat_map(|run| &run.glyphs)
                .filter(|glyph| glyph.index <= begin)
                .max_by_key(|glyph| glyph.index);
            let left = cluster
                .map_or(start_x, |glyph| glyph.position.x)
                .max(start_x);
            let right = layout.x_for_index(end).min(end_x);
            let offset = match align {
                TextAlign::Left => px(0.0),
                TextAlign::Center => (width - (end_x - start_x)) / 2.0,
                TextAlign::Right => width - (end_x - start_x),
            };
            if right > left {
                clips.push(Bounds::new(
                    point(
                        origin.x + offset + left - start_x,
                        origin.y + height * row as f32,
                    ),
                    size(right - left, height),
                ));
            }
        }
        start_byte = end_byte;
        start_x = end_x;
    }
    clips
}

#[cfg(test)]
mod tests;
